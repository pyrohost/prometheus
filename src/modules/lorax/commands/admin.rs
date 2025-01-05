//! Commands for managing Lorax events.

use std::sync::Arc;

use crate::modules::lorax::database::LoraxEvent;
use crate::modules::lorax::{database::LoraxStage, task::LoraxEventTask};
use crate::{Context, Error};
use poise::command;
use poise::serenity_prelude::{self as serenity, ChannelId, EditMessage, Mentionable};
use tracing::error;

/// Kick off a new Lorax event for your community!
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn start(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;

    let guild_id = ctx.guild_id().unwrap().get();

    if let Some(event) = ctx.data().dbs.lorax.get_event(guild_id).await {
        if event.stage != LoraxStage::Inactive {
            ctx.say("❌ There is already an active event!").await?;
            return Ok(());
        }
    }

    let settings = ctx.data().dbs.lorax.get_settings(guild_id).await?;

    if settings.lorax_channel.is_none() {
        ctx.say("❌ Please set a Lorax channel first using `/lorax channel`")
            .await?;
        return Ok(());
    }

    let mut lorax_task = LoraxEventTask::new(guild_id, Arc::new(ctx.data().dbs.lorax.clone()));

    lorax_task
        .start_event(settings, ctx.serenity_context())
        .await;

    ctx.say("🎉 The Lorax event has begun! Let the naming commence!")
        .await?;
    Ok(())
}

/// Wrap up the current Lorax event
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn end(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();
    let mut lorax_task = LoraxEventTask::new(guild_id, Arc::new(ctx.data().dbs.lorax.clone()));

    match lorax_task.end_event(ctx.serenity_context()).await {
        Ok(_) => {
            ctx.say("🛑 The Lorax event has been ended. Thanks for participating!")
                .await?;
        }
        Err(e) => {
            ctx.say(format!("❌ {}", e)).await?;
        }
    }

    Ok(())
}

async fn handle_winner_roles(
    ctx: &serenity::Context,
    guild_id: u64,
    event: &LoraxEvent,
    winner_tree: &str,
) -> Result<(), Error> {
    let settings = &event.settings;
    
    // Get winner and previous winner roles
    if let (Some(winner_role_id), Some(alumni_role_id)) = (settings.winner_role, settings.alumni_role) {
        let winner_role = serenity::RoleId::from(winner_role_id);
        let alumni_role = serenity::RoleId::from(alumni_role_id);

        // Move current winners to alumni
        if let Ok(guild) = ctx.http.get_guild(guild_id.into()).await {
            if let Ok(members) = guild.members(ctx, None, None).await {
                for member in members {
                    if member.roles.contains(&winner_role) {
                        let _ = member.remove_role(ctx, winner_role).await;
                        let _ = member.add_role(ctx, alumni_role).await;
                    }
                }

                // Assign winner role to new winner
                if let Some(winner_id) = event.get_tree_submitter(winner_tree) {
                    if let Ok(winner) = guild.member(ctx, winner_id).await {
                        let _ = winner.add_role(ctx, winner_role).await;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Skip to the next event stage
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn force_advance(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    if matches!(event.stage, LoraxStage::Completed | LoraxStage::Inactive) {
        ctx.say("❌ Cannot advance a completed or inactive event.")
            .await?;
        return Ok(());
    }

    let mut updated_event = event.clone();
    let mut lorax_task = LoraxEventTask::new(guild_id, Arc::new(ctx.data().dbs.lorax.clone()));

    if matches!(updated_event.stage, LoraxStage::Voting) {
        // Handle role assignments before advancing
        if let Some(winner_tree) = updated_event.get_winner() {
            handle_winner_roles(ctx.serenity_context(), guild_id, &updated_event, &winner_tree).await?;
        }
    }

    lorax_task
        .advance_stage(ctx.serenity_context(), &mut updated_event)
        .await;

    if !matches!(updated_event.stage, LoraxStage::Inactive) {
        if let Err(e) = ctx
            .data()
            .dbs
            .lorax
            .update_event(guild_id, updated_event)
            .await
        {
            error!("Failed to update event for guild {}: {}", guild_id, e);
            ctx.say("❌ Failed to update event stage. Please try again later.")
                .await?;
        } else {
            ctx.say("⏩ Advanced to the next stage!").await?;
        }
    }

    Ok(())
}

/// Adjust the current stage duration
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn duration(
    ctx: Context<'_>,
    #[description = "Minutes to add or remove (negative to reduce)"] minutes: i64,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let mut event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    if matches!(event.stage, LoraxStage::Completed | LoraxStage::Inactive) {
        ctx.say("❌ Cannot modify duration of a completed or inactive event.")
            .await?;
        return Ok(());
    }

    let lorax_task = LoraxEventTask::new(guild_id, Arc::new(ctx.data().dbs.lorax.clone()));
    let current_duration = lorax_task.calculate_stage_duration(&event);

    let adjusted_duration = (current_duration as i64) + (minutes * 60);
    if adjusted_duration < 0 {
        ctx.say("❌ Duration cannot be negative.").await?;
        return Ok(());
    }

    let new_duration = adjusted_duration as u64;
    lorax_task.adjust_stage_duration(&mut event, new_duration);

    if let Some(channel_id) = event.settings.lorax_channel {
        let change_type = if minutes > 0 { "extended" } else { "reduced" };
        let msg = format!(
            "⏰ Event stage has been {} by {} minutes! New end time: <t:{}:R>",
            change_type,
            minutes.abs(),
            event.get_stage_end_timestamp(new_duration)
        );

        let channel = ChannelId::new(channel_id);
        channel.say(&ctx.serenity_context().http, &msg).await?;

        if let Some(msg_id) = match event.stage {
            LoraxStage::Submission => event.stage_message_id,
            LoraxStage::Voting => event.voting_message_id,
            LoraxStage::Tiebreaker(_) => event.tiebreaker_message_id,
            _ => None,
        } {
            if let Ok(mut message) = channel.message(&ctx.serenity_context().http, msg_id).await {
                let new_content = message.content.replace(
                    r"<t:\d+:R>",
                    &format!("<t:{}:R>", event.get_stage_end_timestamp(new_duration)),
                );
                let _ = message
                    .edit(
                        &ctx.serenity_context().http,
                        EditMessage::new().content(new_content),
                    )
                    .await;
            }
        }
    }

    let _ = ctx.data().dbs.lorax.update_event(guild_id, event).await;
    ctx.say(format!(
        "⏳ Stage duration adjusted by {} minutes.",
        minutes
    ))
    .await?;
    Ok(())
}

/// Set event phase durations
#[command(slash_command, guild_only)]
pub async fn durations(
    ctx: Context<'_>,
    #[description = "Minutes for submissions"] submission: Option<u64>,
    #[description = "Minutes for voting"] voting: Option<u64>,
    #[description = "Minutes for tiebreakers"] tiebreaker: Option<u64>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    if submission.is_none() && voting.is_none() && tiebreaker.is_none() {
        ctx.say("❌ Please specify at least one duration to update.")
            .await?;
        return Ok(());
    }

    match ctx
        .data()
        .dbs
        .lorax
        .transaction(|db| {
            let settings = db.settings.entry(guild_id).or_default();
            if let Some(mins) = submission {
                settings.submission_duration = mins;
            }
            if let Some(mins) = voting {
                settings.voting_duration = mins;
            }
            if let Some(mins) = tiebreaker {
                settings.tiebreaker_duration = mins;
            }
            Ok(())
        })
        .await
    {
        Ok(_) => {
            ctx.say("⏱️ Durations updated!").await?;
        }
        Err(e) => {
            error!("Failed to update durations for guild {}: {}", guild_id, e);
            ctx.say("❌ Failed to update durations. Please try again later.")
                .await?;
        }
    }

    Ok(())
}

/// Reset Lorax settings and events
#[command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn reset(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    match ctx
        .data()
        .dbs
        .lorax
        .transaction(|db| {
            db.events.remove(&guild_id);
            db.settings.remove(&guild_id);
            Ok(())
        })
        .await
    {
        Ok(_) => {
            ctx.say("🔄 Lorax has been reset for this server.").await?;
        }
        Err(e) => {
            error!("Failed to reset Lorax for guild {}: {}", guild_id, e);
            ctx.say("❌ Failed to reset Lorax settings. Please try again later.")
                .await?;
        }
    }

    Ok(())
}

const ITEMS_PER_PAGE: usize = 12;

/// View all submissions and who submitted them
#[command(slash_command, guild_only, ephemeral)]
pub async fn submissions(
    ctx: Context<'_>,
    #[description = "Page number to view"] page: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();
    let page = page.unwrap_or(1).max(1);

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    let has_manage_messages = ctx.author_member().await.map_or(false, |m| {
        ctx.guild().map_or(false, |g| {
            let channel = g.channels.values().next().unwrap();
            g.user_permissions_in(channel, &m).manage_messages()
        })
    });

    if matches!(event.stage, LoraxStage::Submission) && !has_manage_messages {
        ctx.say("❌ Cannot view submissions during the submission phase.")
            .await?;
        return Ok(());
    }

    let mut submissions: Vec<_> = event
        .tree_submissions
        .iter()
        .map(|(user_id, tree)| (tree.clone(), *user_id))
        .collect();
    submissions.sort_by(|a, b| a.0.cmp(&b.0));

    let total_pages = (submissions.len() + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE;
    if total_pages == 0 {
        ctx.say("📝 No submissions yet!").await?;
        return Ok(());
    }

    let current_page = page.min(total_pages);
    let start = (current_page - 1) * ITEMS_PER_PAGE;
    let end = (start + ITEMS_PER_PAGE).min(submissions.len());

    let entries: Vec<_> = submissions[start..end]
        .iter()
        .map(|(tree, user_id)| format!("\"{}\" by <@{}>", tree, user_id))
        .collect();

    let msg = format!(
        "📋 **All Submissions ({} total)**\nPage {}/{}\n\n{}",
        submissions.len(),
        current_page,
        total_pages,
        entries.join("\n")
    );

    ctx.say(msg).await?;
    Ok(())
}

/// View current vote counts for each tree
#[command(slash_command, guild_only, ephemeral)]
pub async fn votes(
    ctx: Context<'_>,
    #[description = "Page number to view"] page: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();
    let page = page.unwrap_or(1).max(1);

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    let has_manage_messages = ctx.author_member().await.map_or(false, |m| {
        ctx.guild().map_or(false, |g| {
            let channel = g.channels.values().next().unwrap();
            g.user_permissions_in(channel, &m).manage_messages()
        })
    });

    if !matches!(event.stage, LoraxStage::Completed) && !has_manage_messages {
        ctx.say("❌ Votes can only be viewed after the event is completed.")
            .await?;
        return Ok(());
    }

    let total_votes = event.tree_votes.len();

    let mut vote_counts: std::collections::HashMap<String, (usize, Option<u64>)> =
        std::collections::HashMap::new();
    
    // Count votes and track submitters
    for tree in event.tree_votes.values() {
        let entry = vote_counts.entry(tree.clone()).or_insert((0, event.get_tree_submitter(tree)));
        entry.0 += 1;
    }

    let mut vote_counts: Vec<_> = vote_counts.into_iter().collect();
    vote_counts.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));

    if vote_counts.is_empty() {
        ctx.say("📝 No votes cast yet!").await?;
        return Ok(());
    }

    let total_pages = (vote_counts.len() + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE;
    let current_page = page.min(total_pages);
    let start = (current_page - 1) * ITEMS_PER_PAGE;
    let end = (start + ITEMS_PER_PAGE).min(vote_counts.len());

    let entries: Vec<String> = vote_counts[start..end]
        .iter()
        .enumerate()
        .map(|(i, (tree, (count, submitter)))| {
            let rank = start + i + 1;
            let medal = match rank {
                1 => "🥇",
                2 => "🥈",
                3 => "🥉",
                _ => "  ",
            };
            let percentage = (*count as f64 / total_votes as f64) * 100.0;
            let submitter_text = submitter
                .map(|uid| format!(" (by <@{}>)", uid))
                .unwrap_or_default();
            
            format!(
                "{} **{}**{} - {} votes ({:.1}%)",
                medal, tree, submitter_text, count, percentage
            )
        })
        .collect();

    let msg = format!(
        "🗳️ **Current Vote Counts ({} total votes)**\nPage {}/{}\n\n{}",
        total_votes,
        current_page,
        total_pages,
        entries.join("\n")
    );

    ctx.say(msg).await?;
    Ok(())
}

/// Remove a submission from the event
#[command(slash_command, guild_only, required_permissions = "MANAGE_MESSAGES")]
pub async fn remove_submission(
    ctx: Context<'_>,
    #[description = "Tree name to remove"] tree: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let mut event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    let tree = tree.to_lowercase();
    let submitter = event
        .get_tree_submitter(&tree)
        .ok_or("That tree name was not found")?;

    event.tree_submissions.remove(&submitter);
    event.eliminated_trees.insert(tree.clone());

    event.tree_votes.retain(|_, voted_tree| voted_tree != &tree);

    let _ = ctx.data().dbs.lorax.update_event(guild_id, event).await;
    ctx.say(format!(
        "🗑️ Removed submission \"{}\" and any related votes.",
        tree
    ))
    .await?;
    Ok(())
}

/// Remove a user's vote
#[command(slash_command, guild_only, required_permissions = "MANAGE_MESSAGES")]
pub async fn remove_vote(
    ctx: Context<'_>,
    #[description = "User to remove vote from"] user: serenity::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let mut event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    if event.tree_votes.remove(&user.id.get()).is_some() {
        let _ = ctx.data().dbs.lorax.update_event(guild_id, event).await;
        ctx.say(format!("🗑️ Removed vote from {}.", user.mention()))
            .await?;
    } else {
        ctx.say(format!("❌ {} has not voted.", user.mention()))
            .await?;
    }
    Ok(())
}
