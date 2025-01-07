use std::{num::NonZero, sync::{Arc, atomic::AtomicBool}};
use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use poise::serenity_prelude::{ChannelId, Context, CreateMessage, FullEvent};
use songbird::{
    events::{EventContext, EventHandler as VoiceEventHandler}, 
    id::{ChannelId as SongbirdChannelId, GuildId as SongbirdGuildId}, 
    input::{codecs::*, Input}, 
    model::{id::UserId, payload::Speaking}, 
    tracks::Track, 
    Call, CoreEvent, Event
};
use tokio::sync::Mutex;
use tracing::{error, info};
use crate::{
    database::Database,
    events::{self, EventHandler},
};
use super::database::{RecordingDatabase, RecordingChannel};

#[derive(Clone)]
struct RecordingReceiver {
    inner: Arc<InnerReceiver>,
}

struct InnerReceiver {
    last_tick_was_empty: AtomicBool,
    known_ssrcs: DashMap<u32, UserId>,
    buffer: Arc<Mutex<Vec<f32>>>,
}

impl InnerReceiver {
    fn convert_samples(samples: &[i16]) -> Vec<f32> {
        samples.iter()
            .map(|&s| (s as f32) / (i16::MAX as f32))
            .collect()
    }
}

impl RecordingReceiver {
    fn new() -> Self {
        Self {
            inner: Arc::new(InnerReceiver {
                last_tick_was_empty: AtomicBool::default(),
                known_ssrcs: DashMap::new(),
                buffer: Arc::new(Mutex::new(Vec::new())),
            }),
        }
    }
}

#[async_trait]
impl VoiceEventHandler for RecordingReceiver {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(Speaking { speaking: _, ssrc, user_id, .. }) => {
                if let Some(user) = user_id {
                    self.inner.known_ssrcs.insert(*ssrc, *user);
                }
            },
            EventContext::VoiceTick(tick) => {
                let speaking = tick.speaking.len();
                if speaking > 0 {
                    for (_ssrc, data) in &tick.speaking {
                        if let Some(decoded_voice) = data.decoded_voice.as_ref() {
                            let mut buffer = self.inner.buffer.lock().await;
                            buffer.extend(InnerReceiver::convert_samples(decoded_voice));
                        }
                    }
                } else if !tick.speaking.is_empty() {
                    // Process accumulated audio when no one is speaking
                    let buffer = self.inner.buffer.lock().await;
                    if !buffer.is_empty() {
                        info!("Received {} samples of audio data", buffer.len());
                        // TODO: Save audio data to file
                    }
                }
            },
            _ => {},
        }
        None
    }
}

#[derive(Debug)]
pub struct RecordingHandler {
    db: Database<RecordingDatabase>,
}

impl RecordingHandler {
    pub fn new(db: Database<RecordingDatabase>) -> Self {
        Self { db }
    }

    async fn create_track(bytes: Vec<u8>) -> Result<Track, Box<dyn std::error::Error + Send + Sync>> {
        // Create input directly from bytes
        let input = Input::from(bytes);
        
        // Make it playable and create track
        let input = input.make_playable_async(&CODEC_REGISTRY, &PROBE).await?;
        Ok(Track::from(input))
    }

    async fn play_intro_sounds(&self, ctx: &Context, channel: &RecordingChannel) {
        let manager = songbird::get(ctx).await.expect("Songbird not initialized");
        
        if let Some(handler_lock) = manager.get(SongbirdGuildId(NonZero::new(channel.guild_id).unwrap())) {
            let mut handler = handler_lock.lock().await;

            // Play start sound
            let start_bytes = include_bytes!("../../../extra/recording-start.mp3").to_vec();
            if let Ok(track) = Self::create_track(start_bytes).await {
                let handle = handler.play(track);
                handle.set_volume(1.0).expect("Failed to set volume");

                // Wait for sound to finish
                loop {
                    if let Ok(info) = handle.get_info().await {
                        if info.playing.is_done() {
                            break;
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
            
            // Play voice sound
            let voice_bytes = include_bytes!("../../../extra/recording-voice.wav").to_vec();
            if let Ok(track) = Self::create_track(voice_bytes).await {
                let handle = handler.play(track);
                handle.set_volume(1.0).expect("Failed to set volume");
            }
        }
    }

    async fn notify_channel(&self, ctx: &Context, channel: &RecordingChannel, msg: &str) {
        let voice_channel = ChannelId::from(channel.voice_channel_id);
        if let Ok(channel) = voice_channel.to_channel(&ctx).await {
            if let Some(text_id) = channel.guild().and_then(|c| Some(c.id)) {
                if let Err(e) = text_id.say(&ctx.http, msg).await {
                    error!("Failed to send notification: {}", e);
                }
            }
        }
    }

    async fn handle_recording_stop(&self, ctx: &Context, channel: &RecordingChannel, handler_lock: Arc<Mutex<Call>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut handler = handler_lock.lock().await;
        
        let receiver = RecordingReceiver::new();
        handler.remove_all_global_events();
        handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());
        handler.add_global_event(CoreEvent::VoiceTick.into(), receiver.clone());
        
        // Get text channel from voice channel
        let voice_channel = ChannelId::from(channel.voice_channel_id);
        if let Ok(channel) = voice_channel.to_channel(&ctx).await {
            if let Some(text_id) = channel.guild().and_then(|c| c.parent_id) {
                text_id.send_message(&ctx.http, CreateMessage::default().content("🔄 Uploading recording...")).await?;
            };
        }
        
        Ok(())
    }

    async fn handle(
        &self,
        ctx: &Context, 
        event: &FullEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match event {
            FullEvent::VoiceStateUpdate { old, new } => {
                // Check if this is for a recording channel
                let channel = self.db.read(|data| {
                    data.channels.values()
                        .find(|c| c.voice_channel_id == new.channel_id.map(|c| c.get()).unwrap_or(0))
                        .cloned()
                }).await;

                if let Some(mut channel) = channel {
                    let manager = songbird::get(ctx).await.expect("Songbird not initialized");
                    
                    match (old, new) {
                        // User joined - when going from no channel to a channel
                        (vs_old, vs_new) if vs_new.channel_id.is_some() && vs_old.as_ref().and_then(|s| s.channel_id).is_none() => {
                            if !channel.is_recording {
                                let guild_id = SongbirdGuildId(NonZero::new(channel.guild_id).unwrap());
                                let channel_id = SongbirdChannelId(NonZero::new(channel.voice_channel_id).unwrap());

                                if let Some(handler_lock) = manager.join(guild_id, channel_id).await.ok() {
                                    channel.is_recording = true;
                                    channel.last_activity = Some(Utc::now());
                                    
                                    // Update database
                                    self.db.transaction(|data| {
                                        data.channels.insert(channel.guild_id, channel.clone());
                                        Ok(())
                                    }).await?;
                                    
                                    self.play_intro_sounds(ctx, &channel).await;
                                    
                                    // Start recording
                                    let mut handler = handler_lock.lock().await;
                                    let receiver = RecordingReceiver::new();
                                    handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());
                                    handler.add_global_event(CoreEvent::VoiceTick.into(), receiver);
                                    
                                    self.notify_channel(ctx, &channel, "🎙️ Recording started").await;
                                }
                            }
                        },
                        // User left - when going from a channel to no channel
                        (vs_old, vs_new) if vs_old.as_ref().and_then(|s| s.channel_id).is_some() && vs_new.channel_id.is_none() => {
                            // Extract users count before await
                            let users_in_channel = if let Some(guild) = ctx.cache.guild(channel.guild_id) {
                                guild.voice_states.values()
                                    .filter(|state| state.channel_id == Some(channel.voice_channel_id.into()))
                                    .count()
                            } else {
                                0
                            };
                            
                            if users_in_channel == 0 && channel.is_recording {
                                let guild_id = SongbirdGuildId(NonZero::new(channel.guild_id).unwrap());
                                if let Some(handler_lock) = manager.get(guild_id) {
                                    // Handle recording stop and upload
                                    if let Err(e) = self.handle_recording_stop(ctx, &channel, handler_lock).await {
                                        error!("Failed to handle recording stop: {}", e);
                                    }
                                    
                                    manager.remove(guild_id).await?;
                                    
                                    channel.is_recording = false;
                                    channel.last_activity = Some(Utc::now());
                                    
                                    // Update database
                                    self.db.transaction(|data| {
                                        data.channels.insert(channel.guild_id, channel.clone());
                                        Ok(())
                                    }).await?;
                                    
                                    self.notify_channel(ctx, &channel, "⏹️ Recording stopped").await;
                                }
                            }
                        },
                        _ => {}
                    }
                }
            },
            _ => {}
        }
        
        Ok(())
    }
}

#[async_trait]
impl events::EventHandler for RecordingHandler {
    fn name(&self) -> &str {
        "Recording"
    }
    
    async fn handle(
        &self,
        ctx: &Context, 
        event: &FullEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match event {
            FullEvent::VoiceStateUpdate { old, new } => {
                // Check if this is for a recording channel
                let channel = self.db.read(|data| {
                    data.channels.values()
                        .find(|c| c.voice_channel_id == new.channel_id.map(|c| c.get()).unwrap_or(0))
                        .cloned()
                }).await;

                if let Some(mut channel) = channel {
                    let manager = songbird::get(ctx).await.expect("Songbird not initialized");
                    
                    match (old, new) {
                        // User joined - when going from no channel to a channel
                        (vs_old, vs_new) if vs_new.channel_id.is_some() && vs_old.as_ref().and_then(|s| s.channel_id).is_none() => {
                            if !channel.is_recording {
                                let guild_id = SongbirdGuildId(NonZero::new(channel.guild_id).unwrap());
                                let channel_id = SongbirdChannelId(NonZero::new(channel.voice_channel_id).unwrap());

                                if let Some(handler_lock) = manager.join(guild_id, channel_id).await.ok() {
                                    channel.is_recording = true;
                                    channel.last_activity = Some(Utc::now());
                                    
                                    // Update database
                                    self.db.transaction(|data| {
                                        data.channels.insert(channel.guild_id, channel.clone());
                                        Ok(())
                                    }).await?;
                                    
                                    self.play_intro_sounds(ctx, &channel).await;
                                    
                                    // Start recording
                                    self.notify_channel(ctx, &channel, "🎙️ Recording started").await;
                                }
                            }
                        },
                        // User left - when going from a channel to no channel
                        (vs_old, vs_new) if vs_old.as_ref().and_then(|s| s.channel_id).is_some() && vs_new.channel_id.is_none() => {
                            // Extract users count before await
                            let users_in_channel = if let Some(guild) = ctx.cache.guild(channel.guild_id) {
                                guild.voice_states.values()
                                    .filter(|state| state.channel_id == Some(channel.voice_channel_id.into()))
                                    .count()
                            } else {
                                0
                            };
                            
                            if users_in_channel == 0 && channel.is_recording {
                                let guild_id = SongbirdGuildId(NonZero::new(channel.guild_id).unwrap());
                                if let Some(handler_lock) = manager.get(guild_id) {
                                    // Handle recording stop and upload
                                    if let Err(e) = self.handle_recording_stop(ctx, &channel, handler_lock).await {
                                        error!("Failed to handle recording stop: {}", e);
                                    }
                                    
                                    manager.remove(guild_id).await?;
                                    
                                    channel.is_recording = false;
                                    channel.last_activity = Some(Utc::now());
                                    
                                    // Update database
                                    self.db.transaction(|data| {
                                        data.channels.insert(channel.guild_id, channel.clone());
                                        Ok(())
                                    }).await?;
                                    
                                    self.notify_channel(ctx, &channel, "⏹️ Recording stopped").await;
                                }
                            }
                        },
                        _ => {}
                    }
                }
            },
            _ => {}
        }
        
        Ok(())
    }

    fn box_clone(&self) -> Box<dyn EventHandler> {
        Box::new(Self {
            db: self.db.clone()
        })
    }
}
