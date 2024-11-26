use poise::serenity_prelude::{ChannelId, Context, CreateMessage, CreateThread, Message, GuildId, RoleId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::error;

use crate::{
    database::Database,
    modules::lorax::database::LoraxDatabase,
    tasks::Task,
    modules::lorax::database::LoraxSettings,
};

use super::database::{LoraxEvent, LoraxStage};

pub fn get_current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Clone)]
pub enum LoraxMessage {
    EventStart(LoraxEvent),
    StageUpdate(LoraxEvent),
    DurationChange {
        event: LoraxEvent,
        minutes: i64,
        new_duration: u64
    },
    EventEnd,
}

#[derive(Clone)]
pub struct MessageTask {
    pub guild_id: u64,
    pub message: LoraxMessage,
    db: Database<LoraxDatabase>, // Add database field
}

impl MessageTask {
    fn new(guild_id: u64, message: LoraxMessage, db: Database<LoraxDatabase>) -> Self {
        Self {
            guild_id,
            message,
            db,
        }
    }

    async fn execute(&self, ctx: &Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match &self.message {
            LoraxMessage::EventStart(event) | LoraxMessage::StageUpdate(event) => {
                if let Some(channel_id) = event.settings.lorax_channel {
                    let duration = match event.stage {
                        LoraxStage::Submission => event.settings.submission_duration * 60,
                        LoraxStage::Voting => event.settings.voting_duration * 60,
                        LoraxStage::Tiebreaker(_) => event.settings.tiebreaker_duration * 60,
                        _ => 0,
                    };
                    let end_timestamp = event.get_stage_end_timestamp(duration);
                    let message = LoraxEventTask::generate_stage_message(event, end_timestamp);

                    let channel = ChannelId::new(channel_id);
                    let _ = channel.send_message(&ctx.http, CreateMessage::new().content(message)).await;
                }
            },
            LoraxMessage::DurationChange { event, minutes, new_duration } => {
                if let Some(channel_id) = event.settings.lorax_channel {
                    let change_type = if *minutes > 0 { "extended" } else { "reduced" };
                    let msg = format!(
                        "⏰ Event stage {} by {} minutes! New end time: <t:{}:R>",
                        change_type, minutes.abs(), 
                        event.get_stage_end_timestamp(*new_duration)
                    );
                    
                    let channel = ChannelId::new(channel_id);
                    let _ = channel.send_message(&ctx.http, CreateMessage::new().content(msg)).await;
                }
            },
            LoraxMessage::EventEnd => {
                if let Some(event) = self.db.get_event(self.guild_id).await {
                    if let Some(channel_id) = event.settings.lorax_channel {
                        let mut end_event = event.clone();
                        end_event.stage = LoraxStage::Completed;
                        let message = LoraxEventTask::generate_stage_message(&end_event, 0);
                        
                        let channel = ChannelId::new(channel_id);
                        let _ = channel.send_message(&ctx.http, CreateMessage::new().content(message)).await;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct LoraxEventTask {
    pub guild_id: u64,
    pub db: Database<LoraxDatabase>,
    message_queue: Arc<Mutex<Vec<MessageTask>>>,
}

impl LoraxEventTask {
    pub fn new(guild_id: u64, db: Database<LoraxDatabase>) -> Self {
        Self {
            guild_id,
            db,
            message_queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn calculate_stage_duration(&self, event: &LoraxEvent) -> u64 {
        match event.stage {
            LoraxStage::Submission => event.settings.submission_duration * 60,
            LoraxStage::Voting => event.settings.voting_duration * 60,
            LoraxStage::Tiebreaker(_) => event.settings.tiebreaker_duration * 60,
            _ => 0,
        }
    }

    async fn create_campaign_thread(
        &self,
        ctx: &Context,
        channel_id: u64,
        msg: &Message,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let thread = ChannelId::new(channel_id)
            .create_thread_from_message(
                &ctx.http,
                msg.id,
                CreateThread::new("Campaign Thread")
                    .auto_archive_duration(poise::serenity_prelude::AutoArchiveDuration::OneDay)
                    .kind(poise::serenity_prelude::ChannelType::PublicThread),
            )
            .await?;

        let campaign_msg = format!(
            "🌳 Welcome to the campaign thread!\n\
            If you submitted a tree name, you can campaign for it here.\n\
            Share why your tree name should win and convince others to vote for it!"
        );
        thread
            .send_message(&ctx.http, CreateMessage::new().content(campaign_msg))
            .await?;
        Ok(thread.id.get())
    }

    async fn send_stage_message(&self, ctx: &Context, event: &mut LoraxEvent) {
        if let Some(channel_id) = event.settings.lorax_channel {
            let duration = self.calculate_stage_duration(event);
            let end_timestamp = event.get_stage_end_timestamp(duration);
            let message = Self::generate_stage_message(event, end_timestamp);

            let channel = ChannelId::new(channel_id);
            if let Ok(msg) = channel.send_message(&ctx.http, CreateMessage::new().content(&message)).await {
                event.update_stage_message_id(msg.id.get());

                // Create campaign thread for voting phase
                if matches!(event.stage, LoraxStage::Voting) {
                    if let Ok(thread_id) = self.create_campaign_thread(ctx, channel_id, &msg).await {
                        event.campaign_thread_id = Some(thread_id);
                    }
                }
            }
        }
    }

    fn generate_stage_message(event: &LoraxEvent, end_timestamp: u64) -> String {
        let role_mention = event.settings.lorax_role
            .map(|id| format!("<@&{}>", id))
            .unwrap_or_default();

        match event.stage {
            LoraxStage::Submission => format!(
                "{} 🌳 **A New Lorax Event Has Started!**\n\n\
                🌿 Use `/lorax submit` to suggest a tree name for our new node!\n\
                ⏰ Submission phase ends <t:{}:R>\n\n\
                💡 Be creative and unique with your submission!",
                role_mention, end_timestamp
            ),
            LoraxStage::Voting => format!(
                "{} 🗳️ **Submission Phase Has Ended!**\n\n\
                🌿 Use `/lorax vote` to pick your favorite node name!\n\
                ⏰ Voting ends <t:{}:R>\n\n\
                💭 Consider joining the discussion in the campaign thread!",
                role_mention, end_timestamp
            ),
            LoraxStage::Tiebreaker(round) => format!(
                "{} 🎯 **Tiebreaker Round {}!**\n\n\
                📝 Use `/lorax vote` to break the tie!\n\
                ⏰ Tiebreaker ends <t:{}:R>",
                role_mention, round, end_timestamp
            ),
            LoraxStage::Completed => {
                let winner = event.current_trees.first().map_or("Unknown", |s| s.as_str());
                let mut msg = format!(
                    "{} ✨ **The Lorax Event Has Concluded!**\n\n\
                    👑 **Our new node will be named: {}**",
                    role_mention, winner
                );

                if event.current_trees.len() > 1 {
                    msg.push_str("\n\n🏆 **Final Results:**");
                    for (i, tree) in event.current_trees.iter().take(3).enumerate() {
                        let medal = match i {
                            0 => "🥇",
                            1 => "🥈",
                            2 => "🥉",
                            _ => unreachable!(),
                        };
                        msg.push_str(&format!("\n{} {}", medal, tree));
                    }

                    if event.current_trees.len() > 3 {
                        let runner_ups = event.current_trees.len() - 3;
                        msg.push_str(&format!("\n\n... and {} other runner-up{}!", 
                            runner_ups,
                            if runner_ups == 1 { "" } else { "s" }
                        ));
                    }
                }

                msg.push_str("\n\nThank you all for participating! 🌿");
                msg
            },
            LoraxStage::Inactive => String::new(),
        }
    }

    async fn manage_winner_roles(
        &self,
        ctx: &Context,
        guild_id: u64,
        winner_id: u64,
        settings: &LoraxSettings,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let guild = GuildId::new(guild_id);

        // If we have a winner role configured
        if let Some(winner_role) = settings.winner_role {
            let winner_role_id = RoleId::new(winner_role);
            
            // Remove winner role from all members who have it
            if let Ok(members) = guild.members(&ctx.http, None, None).await {
                for member in members {
                    if member.roles.contains(&winner_role_id) {
                        // If alumni role exists, add it before removing winner role
                        if let Some(alumni_role) = settings.alumni_role {
                            let _ = member.add_role(&ctx.http, RoleId::new(alumni_role)).await;
                        }
                        let _ = member.remove_role(&ctx.http, winner_role_id).await;
                    }
                }
            }

            // Add winner role to new winner
            if let Ok(winner) = guild.member(&ctx.http, winner_id).await {
                let _ = winner.add_role(&ctx.http, winner_role_id).await;
            }
        }

        Ok(())
    }

    pub async fn advance_stage(&self, ctx: &Context, event: &mut LoraxEvent) {
        // Don't try to advance if already completed
        if matches!(event.stage, LoraxStage::Completed | LoraxStage::Inactive) {
            return;
        }

        let continues = event.advance_stage();
        event.start_time = get_current_timestamp();
        
        if continues {
            // Only send stage message directly if continuing
            self.send_stage_message(ctx, event).await;
        } else {
            // Handle winner roles before completing the event
            if let Some(winner_name) = event.current_trees.first() {
                if let Some(winner_id) = event.get_tree_submitter(winner_name) {
                    if let Err(e) = self.manage_winner_roles(ctx, self.guild_id, winner_id, &event.settings).await {
                        error!("Failed to manage winner roles: {}", e);
                    }
                }
            }

            // Existing completion logic
            event.stage = LoraxStage::Completed;
            self.queue_message(LoraxMessage::StageUpdate(event.clone())).await;
            self.process_message_queue(ctx).await;
            
            // Now mark as inactive and remove from database
            event.stage = LoraxStage::Inactive;
            self.db.write(|db| {
                db.events.remove(&self.guild_id);
            }).await;
        }
    }

    pub async fn queue_message(&self, message: LoraxMessage) {
        let task = MessageTask::new(self.guild_id, message, self.db.clone());
        let mut queue = self.message_queue.lock().await;
        queue.push(task);
    }

    async fn process_message_queue(&self, ctx: &Context) {
        let mut queue = self.message_queue.lock().await;
        for task in queue.drain(..) {
            if let Err(e) = task.execute(ctx).await {
                error!("Failed to process message task: {}", e);
            }
        }
    }

    pub async fn start_event(&mut self, guild_id: u64, settings: LoraxSettings, ctx: &Context) {
        self.guild_id = guild_id;
        let event = LoraxEvent::new(settings.clone(), get_current_timestamp());
        
        // Queue the start message before updating DB
        self.queue_message(LoraxMessage::EventStart(event.clone())).await;
        self.db.update_event(guild_id, event).await;
        
        // Process messages immediately
        self.process_message_queue(ctx).await;
    }

    pub async fn end_event(&mut self, guild_id: u64, ctx: &Context) -> Result<(), String> {
        if let Some(mut event) = self.db.get_event(guild_id).await {
            // Set to completed first to ensure proper message
            event.stage = LoraxStage::Completed;
            
            // Queue and send the completion message
            self.queue_message(LoraxMessage::StageUpdate(event.clone())).await;
            self.process_message_queue(ctx).await;
            
            // Remove the event
            self.db.write(|db| {
                db.events.remove(&guild_id);
            }).await;
            Ok(())
        } else {
            Err("No active event found".to_string())
        }
    }
}

impl LoraxEvent {
    pub fn advance_stage(&mut self) -> bool {
        match self.stage {
            LoraxStage::Submission => {
                if self.tree_submissions.is_empty() {
                    return false;
                }
                self.current_trees = self.tree_submissions.values().cloned().collect();
                self.stage = LoraxStage::Voting;
            }
            LoraxStage::Voting => {
                let vote_counts = count_votes(&self.tree_votes);
                let max_votes = vote_counts.values().max().unwrap_or(&0);
                let winners: Vec<_> = vote_counts
                    .iter()
                    .filter(|&(_, &count)| count == *max_votes)
                    .map(|(tree, _)| tree.clone())
                    .collect();

                let has_tie = winners.len() > 1;
                self.current_trees = winners;
                
                if has_tie {
                    self.stage = LoraxStage::Tiebreaker(1);
                    self.tree_votes.clear();
                } else {
                    self.stage = LoraxStage::Completed;
                    return false;
                }
            }
            LoraxStage::Tiebreaker(round) => {
                let vote_counts = count_votes(&self.tree_votes);
                let max_votes = vote_counts.values().max().unwrap_or(&0);
                let winners: Vec<_> = vote_counts
                    .iter()
                    .filter(|&(_, &count)| count == *max_votes)
                    .map(|(tree, _)| tree.clone())
                    .collect();

                let has_tie = winners.len() > 1;
                self.current_trees = winners;
                
                if has_tie {
                    self.stage = LoraxStage::Tiebreaker(round + 1);
                    self.tree_votes.clear();
                } else {
                    self.stage = LoraxStage::Completed;
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    pub fn update_stage_message_id(&mut self, message_id: u64) {
        match self.stage {
            LoraxStage::Submission => self.stage_message_id = Some(message_id),
            LoraxStage::Voting => self.voting_message_id = Some(message_id),
            LoraxStage::Tiebreaker(_) => self.tiebreaker_message_id = Some(message_id),
            _ => {}
        }
    }
}

fn count_votes(votes: &HashMap<u64, String>) -> HashMap<String, u32> {
    let mut vote_counts: HashMap<String, u32> = HashMap::new();
    for vote in votes.values() {
        *vote_counts.entry(vote.clone()).or_insert(0) += 1;
    }
    vote_counts
}

#[async_trait::async_trait]
impl Task for LoraxEventTask {
    fn name(&self) -> &str {
        "LoraxEvent"
    }

    fn schedule(&self) -> Option<Duration> {
        Some(Duration::from_secs(30)) // Check every 30 seconds
    }

    async fn execute(&self, ctx: &Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current_time = get_current_timestamp();
        
        // Process message queue first
        self.process_message_queue(ctx).await;

        let event = match self.db.get_event(self.guild_id).await {
            Some(event) => event,
            None => return Ok(()),
        };

        let stage_duration = self.calculate_stage_duration(&event);
        if current_time - event.start_time >= stage_duration {
            let mut updated_event = event.clone();
            self.advance_stage(ctx, &mut updated_event).await;
            self.db.update_event(self.guild_id, updated_event).await;
        }

        Ok(())
    }

    fn box_clone(&self) -> Box<dyn Task> {
        Box::new(self.clone())
    }
}
