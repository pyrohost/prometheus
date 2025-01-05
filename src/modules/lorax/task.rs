use crate::{
    database::Database,
    modules::lorax::database::{LoraxDatabase, LoraxEvent, LoraxSettings, LoraxStage},
    tasks::Task,
};
use poise::serenity_prelude::{
    AutoArchiveDuration, ChannelId, ChannelType, Context, CreateAllowedMentions, CreateMessage,
    CreateThread, EditThread, RoleId,
};
use rand::seq::SliceRandom;
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
            _ => {}
        }
    }

    pub async fn start_event(&mut self, settings: LoraxSettings, ctx: &Context) {
        let event = LoraxEvent::new(settings, get_current_timestamp());
        if let Err(e) = self.db.update_event(self.guild_id, event).await {
            tracing::error!("Failed to update event: {}", e);
            return;
        }

        if let Some(mut event) = self.db.get_event(self.guild_id).await {
            event.stage = LoraxStage::Submission;

            let _ = self.db.update_event(self.guild_id, event.clone()).await;

            self.send_stage_message(ctx, &mut event).await;
        }
    }

    fn get_winners(&self, event: &LoraxEvent) -> Vec<(String, usize)> {
        let mut vote_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for tree in event.tree_votes.values() {
            *vote_counts.entry(tree.clone()).or_insert(0) += 1;
        }

        let mut winners: Vec<_> = vote_counts.into_iter().collect();
        winners.sort_by(|a, b| b.1.cmp(&a.1));
        winners
    }

    async fn handle_winner_roles(&self, ctx: &Context, event: &LoraxEvent) {
        let guild_id = self.guild_id.into();

        let winner_role = event.settings.winner_role.map(RoleId::new);
        let alumni_role = event.settings.alumni_role.map(RoleId::new);

        if winner_role.is_none() && alumni_role.is_none() {
            return;
        }

        let winners = self.get_winners(event);
        if winners.is_empty() {
            return;
        }

        if let (Some(winner_role), Some(alumni_role)) = (winner_role, alumni_role) {
            if let Ok(guild) = ctx.http.get_guild(guild_id).await {
                if let Some((winning_tree, _)) = winners.first() {
                    if let Some(winner_id) = event.get_tree_submitter(winning_tree) {
                        if let Ok(member) = guild.member(ctx, winner_id).await {
                            if let Err(e) = member.add_role(ctx, winner_role).await {
                                tracing::error!("Failed to add winner role: {}", e);
                                return;
                            }
                        }
                    }
                }

                let mut after = None;
                while let Ok(members) = guild.members(ctx, Some(1000), after).await {
                    if members.is_empty() {
                        break;
                    }
                    after = members.last().map(|m| m.user.id);

                    for member in members {
                        if member.roles.contains(&winner_role) {
                            let _ = member.remove_role(ctx, winner_role).await;
                            let _ = member.add_role(ctx, alumni_role).await;
                        }
                    }
                }
            }
        }
    }

    pub async fn advance_stage(&mut self, ctx: &Context, event: &mut LoraxEvent) {
        let old_stage = event.stage.clone();
        
        match event.stage {
            LoraxStage::Submission => {
                if event.tree_submissions.is_empty() {
                    event.stage = LoraxStage::Inactive;
                } else {
                    event.stage = LoraxStage::Voting;
                    event.current_trees = event.tree_submissions.values().cloned().collect();
                }
                event.start_time = get_current_timestamp();
            }
            LoraxStage::Voting => {
                if event.tree_votes.is_empty() {
                    event.stage = LoraxStage::Inactive;
                } else {
                    let winners = self.get_winners(event);
                    // Check for ties
                    if winners.len() >= 2 && winners[0].1 == winners[1].1 {
                        event.stage = LoraxStage::Tiebreaker(1);
                        event.current_trees = winners
                            .iter()
                            .take_while(|(_, votes)| votes == &winners[0].1)
                            .map(|(tree, _)| tree.clone())
                            .collect();
                    } else {
                        event.stage = LoraxStage::Completed;
                        event.current_trees = winners.into_iter().map(|(tree, _)| tree).collect();
                        self.handle_winner_roles(ctx, event).await;
                    }
                }
                event.start_time = get_current_timestamp();
                event.tree_votes.clear(); // Reset votes for next stage
            }
            LoraxStage::Tiebreaker(round) => {
                if round >= 3 {
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

        tracing::info!(
            "Advanced Lorax event from {:?} to {:?} for guild {}",
            old_stage,
            event.stage,
            self.guild_id
        );

        if let Err(e) = self.db.update_event(self.guild_id, event.clone()).await {
            tracing::error!("Failed to update event stage: {}", e);
        }
        self.send_stage_message(ctx, event).await;
    }

    pub async fn end_event(&mut self, ctx: &Context) -> Result<(), String> {
        if let Some(mut event) = self.db.get_event(self.guild_id).await {
            event.stage = LoraxStage::Completed;
            self.send_stage_message(ctx, &mut event).await;

            self.db
                .transaction(|db| {
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
        let channel_id = match event.settings.lorax_channel {
            Some(id) => id,
            None => {
                tracing::error!("No Lorax channel configured for guild {}", self.guild_id);
                return;
            }
        };

        // Validate channel exists and is accessible
        let channel = match ctx.http.get_channel(ChannelId::new(channel_id)).await {
            Ok(channel) => channel,
            Err(e) => {
                tracing::error!("Failed to fetch channel {}: {}", channel_id, e);
                return;
            }
        };

        let text_channel = match channel.guild() {
            Some(tc) => tc,
            None => {
                tracing::error!("Channel {} is not a guild text channel", channel_id);
                return;
            }
        };

        let role_ping = event
            .settings
            .lorax_role
            .map(|id| format!("<@&{}> ", id))
            .unwrap_or_default();

        let sample_trees = vec!["Willow", "Sequoia", "Maple", "Oak", "Pine"];
        let random_tree = sample_trees
            .choose(&mut rand::thread_rng())
            .unwrap_or(&"Tree");

        let content = match event.stage {
            LoraxStage::Submission => format!(
                "{role_ping}🌳 Help us name our new node! Submit a tree name like '{random_tree}' with `/lorax submit`.\nSubmissions close <t:{}:R>",
                event.get_stage_end_timestamp(self.calculate_stage_duration(event))
            ),
            LoraxStage::Voting => {
                if event.tree_submissions.is_empty() {
                    event.stage = LoraxStage::Inactive;
                    format!("{role_ping}😕 No tree names were submitted.")
                } else {
                    format!(
                        "{role_ping}🗳️ Time to vote! Use `/lorax vote` to choose the new node's name.\nVoting ends <t:{}:R>",
                        event.get_stage_end_timestamp(self.calculate_stage_duration(event))
                    )
                }
            },
            LoraxStage::Tiebreaker(round) => format!(
                "{role_ping}⚖️ Tiebreaker Round {round}! Vote again with `/lorax vote`.\nEnds <t:{}:R>",
                event.get_stage_end_timestamp(self.calculate_stage_duration(event))
            ),
            LoraxStage::Completed => {
                let mut podium = String::new();
                let total_entries = event.current_trees.len();
                for (i, tree) in event.current_trees.iter().take(3).enumerate() {
                    match i {
                        0 => podium.push_str(&format!("🥇 **{}**", tree)),
                        1 => podium.push_str(&format!("\n🥈 **{}**", tree)),
                        2 => podium.push_str(&format!("\n🥉 **{}**", tree)),
                        _ => unreachable!(),
                    }
                    if let Some(submitter_id) = event.get_tree_submitter(tree) {
                        podium.push_str(&format!(" (by <@{}>)", submitter_id));
                    }
                }
                if total_entries > 3 {
                    podium.push_str(&format!("\n\nand {} runner ups...", total_entries - 3));
                }

                let winner_name = event.current_trees.first()
                    .map(|s| s.as_str())
                    .unwrap_or("Unknown");

                format!(
                    "{role_ping}🎉 **Node Naming Results**\nOur new node will be named **{winner_name}**!\n\n{podium}\n\n🌲 **Event Stats**\n- Names Submitted: {}\n- Votes Cast: {}",
                    event.tree_submissions.len(),
                    event.tree_votes.len()
                )
            },
            LoraxStage::Inactive => return,
        };

        if let Ok(message) = text_channel.send_message(ctx, CreateMessage::default().content(&content).allowed_mentions(
            CreateAllowedMentions::new()
                .roles(vec![event.settings.lorax_role.unwrap_or_default()]),
        )).await {
            match event.stage {
                LoraxStage::Submission => {
                    event.stage_message_id = Some(message.id.get())
                }
                LoraxStage::Voting => {
                    event.voting_message_id = Some(message.id.get());

                    if let Ok(thread) = text_channel
                        .create_thread_from_message(
                            ctx,
                            message.id,
                            CreateThread::new("Campaign Thread")
                                .kind(ChannelType::PublicThread)
                                .auto_archive_duration(AutoArchiveDuration::OneDay),
                        )
                        .await
                    {
                        event.campaign_thread_id = Some(thread.id.get());
                        let welcome_msg = CreateMessage::default()
                            .content("🎭 Welcome to the campaign thread! Tree submitters can campaign for their entries here. Good luck!");
                        let _ = thread.send_message(ctx, welcome_msg).await;
                    }
                }
                LoraxStage::Completed => {
                    if let Some(thread_id) = event.campaign_thread_id {
                        if let Ok(thread) =
                            ctx.http.get_channel(ChannelId::new(thread_id)).await
                        {
                            if let Some(mut thread) = thread.guild() {
                                let _ = thread
                                    .edit_thread(
                                        ctx,
                                        EditThread::new().locked(true).archived(true),
                                    )
                                    .await;
                            }
                        }
                    }
                }
                LoraxStage::Tiebreaker(_) => {
                    event.tiebreaker_message_id = Some(message.id.get())
                }
                _ => {}
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
