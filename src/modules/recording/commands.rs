use crate::Context;
use poise::command;
use poise::serenity_prelude::{ChannelId, ChannelType};
use super::database::RecordingChannel;

/// Enable voice channel recording
#[command(slash_command, guild_only)]
pub async fn enable(
    ctx: Context<'_>,
    #[description = "Voice channel to record"] voice_channel: ChannelId,
) -> Result<(), crate::Error> {
    let guild_id = ctx.guild_id().unwrap();
    
    // Verify channel is voice channel
    let voice_channel_info = voice_channel.to_channel(&ctx).await?;
    
    if voice_channel_info.guild().map(|c| c.kind) != Some(ChannelType::Voice) {
        ctx.say("The specified channel must be a voice channel!").await?;
        return Ok(());
    }
    
    // Get database
    let db = &ctx.data().dbs.recording;
    
    // Check if guild already has a recording channel
    if db.read(|data| {
        data.channels.contains_key(&guild_id.get())
    }).await {
        ctx.say("This guild already has a recording channel set up! Use `/recording disable` first.").await?;
        return Ok(());
    }
    
    // Add recording channel 
    db.transaction(|data| {
        data.channels.insert(
            guild_id.get(),
            RecordingChannel {
                guild_id: guild_id.get(),
                voice_channel_id: voice_channel.get(),
                is_recording: false,
                last_activity: None,
            },
        );
        Ok(())
    })
    .await?;
    
    ctx.say("Voice channel recording enabled!").await?;
    Ok(())
}

/// Disable voice channel recording
#[command(slash_command, guild_only)]
pub async fn disable(
    ctx: Context<'_>,
) -> Result<(), crate::Error> {
    let guild_id = ctx.guild_id().unwrap();
    let db = &ctx.data().dbs.recording;
    
    db.transaction(|data| {
        if data.channels.remove(&guild_id.get()).is_some() {
            Ok(())
        } else {
            Err("No recording channel configured for this guild.".into())
        }
    })
    .await?;
    
    ctx.say("Voice channel recording disabled!").await?;
    Ok(())
}

/// List recording channels
#[command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), crate::Error> {
    let guild_id = ctx.guild_id().unwrap();
    let db = &ctx.data().dbs.recording;
    
    let channel = db.read(|data| {
        data.channels.get(&guild_id.get()).cloned()
    }).await;
    
    match channel {
        Some(channel) => {
            let voice_name = ChannelId::new(channel.voice_channel_id)
                .to_channel(&ctx)
                .await?
                .guild()
                .map(|c| c.name().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
                
            ctx.say(format!(
                "Recording configuration:\nVoice Channel: {}\nCurrently Recording: {}\nLast Activity: {}",
                voice_name,
                if channel.is_recording { "Yes" } else { "No" },
                channel.last_activity.map(|t| t.to_rfc3339()).unwrap_or_else(|| "Never".to_string())
            )).await?;
        }
        None => {
            ctx.say("No recording channel configured for this guild.").await?;
        }
    }
    
    Ok(())
}

/// Toggle voice recording for a channel
#[command(slash_command, guild_only)]
pub async fn toggle(
    ctx: Context<'_>,
    #[description = "Voice channel to record (leave empty to disable)"] voice_channel: Option<ChannelId>,
) -> Result<(), crate::Error> {
    let guild_id = ctx.guild_id().unwrap();
    let db = &ctx.data().dbs.recording;

    match voice_channel {
        Some(channel) => {
            // Verify channel is voice channel
            let channel_info = channel.to_channel(&ctx).await?;
            
            // Check channel type first
            if channel_info.clone().guild().map(|c| c.kind) != Some(ChannelType::Voice) {
                ctx.say("The specified channel must be a voice channel!").await?;
                return Ok(());
            }

            // Update or create recording configuration
            db.transaction(|data| {
                data.channels.insert(
                    guild_id.get(),
                    RecordingChannel {
                        guild_id: guild_id.get(),
                        voice_channel_id: channel.get(),
                        is_recording: false,
                        last_activity: None,
                    },
                );
                Ok(())
            })
            .await?;

            let channel_name = channel_info.guild().map(|c| c.name().to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            ctx.say(format!("Voice recording configured for channel: {}", channel_name)).await?;
        }
        None => {
            // Disable recording if it exists
            db.transaction(|data| {
                if data.channels.remove(&guild_id.get()).is_some() {
                    Ok(())
                } else {
                    Err("No recording channel was configured for this guild.".into())
                }
            })
            .await?;
            
            ctx.say("Voice recording disabled!").await?;
        }
    }
    
    Ok(())
}
