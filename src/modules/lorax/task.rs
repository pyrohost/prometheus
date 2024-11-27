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

#[derive(Clone, Debug)]
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

    pub fn adjust_stage_duration(&self, event: &mut LoraxEvent, duration_sec: u64) {
        let duration_min = duration_sec.saturating_div(60);
        match event.stage {
            LoraxStage::Submission => event.settings.submission_duration = duration_min,
            LoraxStage::Voting => event.settings.voting_duration = duration_min,
            LoraxStage::Tiebreaker(_) => event.settings.tiebreaker_duration = duration_min,
            _ => {},
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
            LoraxStage::Completed => {
                event.stage = LoraxStage::Inactive;
            }
            LoraxStage::Inactive => return,
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
            let elapsed_time = current_time.saturating_sub(event.start_time);

            if elapsed_time > stage_duration {
                let mut updated_event = event.clone();
                self.advance_stage(ctx, &mut updated_event).await;
                let _ = self.db.update_event(self.guild_id, updated_event).await;
            }
        }
    }

    pub async fn send_stage_message(&mut self, ctx: &Context, event: &mut LoraxEvent) {
        if let Some(channel_id) = event.settings.lorax_channel {
            let role_ping = event
                .settings
                .lorax_role
                .map(|id| format!("<@&{}> ", id))
                .unwrap_or_default();

            let content = match event.stage {
                LoraxStage::Submission => format!(
                    "{role_ping}🌿 New Lorax event! Submit your tree name with `/lorax submit`. Submissions close <t:{}:R>",
                    event.get_stage_end_timestamp(self.calculate_stage_duration(event))
                ),
                LoraxStage::Voting => if event.tree_submissions.is_empty() {
                    // If we have no submissions, just move to inactive.
                    event.stage = LoraxStage::Inactive;
                    format!("{role_ping}🗳️ Not enough submissions to start a vote!")
                } else {
                    format!(
                    "{role_ping}🗳️ Time to vote! {} entries submitted. Use `/lorax vote` to choose your favorite. Voting ends <t:{}:R>",
                    event.current_trees.len(),
                    event.get_stage_end_timestamp(self.calculate_stage_duration(event)))
                }
                LoraxStage::Tiebreaker(round) => format!(
                    "{role_ping}🎯 Tiebreaker Round {}! {} entries tied. Vote again with `/lorax vote`. Ends <t:{}:R>",
                    round,
                    event.current_trees.len(),
                    event.get_stage_end_timestamp(self.calculate_stage_duration(event))
                ),
                LoraxStage::Completed => {
                    let mut podium = String::new();
                    for (i, tree) in event.current_trees.iter().take(3).enumerate() {
                        if let Some(winner_id) = event.get_tree_submitter(tree) {
                            let medal = match i {
                                0 => "🥇",
                                1 => "🥈",
                                2 => "🥉",
                                _ => unreachable!(),
                            };
                            podium.push_str(&format!("{} **{}**\n└ Submitted by <@{}>\n\n", medal, tree, winner_id));
                        }
                    }
                    format!(
                        "{role_ping}🎊 **This Lorax Event Has Concluded!**\n\n{}\n🌲 **Event Stats**\n└ Total Entries: {}\n└ Total Votes: {}",
                        podium,
                        event.tree_submissions.len(),
                        event.tree_votes.len()
                    )
                },
                LoraxStage::Inactive => return,
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
