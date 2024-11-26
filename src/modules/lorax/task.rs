use crate::{
    database::Database,
    modules::lorax::database::{LoraxDatabase, LoraxEvent, LoraxSettings, LoraxStage},
    tasks::Task,
};
use poise::serenity_prelude::{ChannelId, Context, CreateMessage};
use std::sync::Arc;
use std::time::Duration;

// Helper functions at top level
pub fn get_current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Clone)]
pub struct LoraxEventTask {
    pub guild_id: u64,
    pub db: Arc<Database<LoraxDatabase>>,
}

impl LoraxEventTask {
    pub fn new(guild_id: u64, db: Arc<Database<LoraxDatabase>>) -> Self {
        Self { guild_id, db }
    }

    pub fn calculate_stage_duration(&self, event: &LoraxEvent) -> u64 {
        match event.stage {
            LoraxStage::Submission => event.settings.submission_duration * 60,
            LoraxStage::Voting => event.settings.voting_duration * 60,
            LoraxStage::Tiebreaker(_) => event.settings.tiebreaker_duration * 60,
            _ => 0,
        }
    }

    pub async fn start_event(&mut self, settings: LoraxSettings, ctx: &Context) {
        let event = LoraxEvent::new(settings, get_current_timestamp());
        let _ = self.db.update_event(self.guild_id, event).await;

        if let Some(mut event) = self.db.get_event(self.guild_id).await {
            event.stage = LoraxStage::Submission;

            let _ = self.db.update_event(self.guild_id, event.clone()).await;

            self.send_stage_message(ctx, &mut event).await;
        }
    }

    pub async fn advance_stage(&mut self, ctx: &Context, event: &mut LoraxEvent) {
        match event.stage {
            LoraxStage::Submission => {
                event.stage = LoraxStage::Voting;
                event.current_trees = event.tree_submissions.values().cloned().collect();
                event.start_time = get_current_timestamp();
            }
            LoraxStage::Voting => {
                event.stage = LoraxStage::Completed;
                event.start_time = get_current_timestamp();
            }
            LoraxStage::Tiebreaker(round) => {
                if (round) >= 3 {
                    event.stage = LoraxStage::Completed;
                } else {
                    event.stage = LoraxStage::Tiebreaker(round + 1);
                }
                event.start_time = get_current_timestamp();
            }
            _ => {}
        }
        self.send_stage_message(ctx, event).await;
    }

    pub async fn end_event(&mut self, ctx: &Context) -> Result<(), String> {
        if let Some(mut event) = self.db.get_event(self.guild_id).await {
            event.stage = LoraxStage::Completed;
            self.send_stage_message(ctx, &mut event).await;

            self.db
                .write(|db| {
                    db.events.remove(&self.guild_id);
                    Ok(())
                })
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("No active event found".to_string())
        }
    }

    pub async fn run(&mut self, ctx: &Context) {
        let current_time = get_current_timestamp();
        if let Some(event) = self.db.get_event(self.guild_id).await {
            if matches!(event.stage, LoraxStage::Inactive) {
                return;
            }

            let stage_duration = self.calculate_stage_duration(&event);
            if current_time - event.start_time >= stage_duration {
                let mut updated_event = event.clone();
                self.advance_stage(ctx, &mut updated_event).await;
                let _ = self.db.update_event(self.guild_id, updated_event).await;
            }
        }
    }

    pub async fn send_stage_message(&mut self, ctx: &Context, event: &mut LoraxEvent) {
        if let Some(channel_id) = event.settings.lorax_channel {
            let content = match event.stage {
                LoraxStage::Submission => format!(
                    "🌿 Submission phase has begun! Use `/lorax submit` to participate.\nEnds <t:{}:R>",
                    event.get_stage_end_timestamp(self.calculate_stage_duration(event))
                ),
                LoraxStage::Voting => format!(
                    "🗳️ Voting phase has started! Use `/lorax vote` to choose your favorite.\nEnds <t:{}:R>",
                    event.get_stage_end_timestamp(self.calculate_stage_duration(event))
                ),
                LoraxStage::Tiebreaker(round) => format!(
                    "🎯 Tiebreaker Round {} has begun! Vote again to break the tie.\nEnds <t:{}:R>",
                    round,
                    event.get_stage_end_timestamp(self.calculate_stage_duration(event))
                ),
                LoraxStage::Completed => "✨ The event has concluded! Thanks for participating!".to_string(),
                LoraxStage::Inactive => "Event is inactive".to_string(),
            };

            if let Ok(channel) = ctx.http.get_channel(ChannelId::new(channel_id)).await {
                if let Some(text_channel) = channel.guild() {
                    let msg = CreateMessage::default().content(&content);
                    if let Ok(message) = text_channel.send_message(ctx, msg).await {
                        match event.stage {
                            LoraxStage::Submission => {
                                event.stage_message_id = Some(message.id.get())
                            }
                            LoraxStage::Voting => event.voting_message_id = Some(message.id.get()),
                            LoraxStage::Tiebreaker(_) => {
                                event.tiebreaker_message_id = Some(message.id.get())
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Task for LoraxEventTask {
    fn name(&self) -> &str {
        "LoraxEvent"
    }

    fn schedule(&self) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }

    async fn execute(
        &mut self,
        ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.run(ctx).await;
        Ok(())
    }

    fn box_clone(&self) -> Box<dyn Task> {
        Box::new(self.clone())
    }
}
