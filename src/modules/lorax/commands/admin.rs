//! Commands for managing Lorax events.

use std::sync::Arc;

use crate::modules::lorax::{database::LoraxStage, task::LoraxEventTask};
use crate::{Context, Error};
use poise::command;
use poise::serenity_prelude::{self as serenity, ChannelId, EditMessage, Mentionable};
use tracing::error;

/// Kick off a new Lorax event for your community!
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn start(ctx: Context<'_>) -> Result<(), Error> {
    // Defer the interaction immediately
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
    // NOTE:we should consider naming our variables like `current_duration_secs`
    // just so its immediately obvious what unit it uses.

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

/// Reset Lorax settings and events
#[command(slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn reset(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    match ctx
        .data()
        .dbs
        .lorax
        .write(|db| {
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

/// View all submissions and who submitted them
#[command(slash_command, guild_only, ephemeral)]
pub async fn submissions(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    // todo: should use member_permissions_in instead of permissions due to deprecation,
    // todo: i coudln't figure out how to use member_permissions_in though lol - ellie
    let has_manage_messages = ctx.author_member().await.map_or(false, |m| {
        m.permissions(ctx.serenity_context())
            .map_or(false, |p| p.manage_messages())
    });

    if matches!(event.stage, LoraxStage::Submission) && !has_manage_messages {
        ctx.say("❌ Cannot view submissions during the submission phase.")
            .await?;
        return Ok(());
    }

    let mut submissions: Vec<_> = event
        .tree_submissions
        .iter()
        .map(|(user_id, tree)| format!("\"{}\" by <@{}>", tree, user_id))
        .collect();
    submissions.sort();

    let msg = if submissions.is_empty() {
        "📝 No submissions yet!".to_string()
    } else {
        format!(
            "📋 **All Submissions ({})**:\n{}",
            submissions.len(),
            submissions.join("\n")
        )
    };

    ctx.say(msg).await?;
    Ok(())
}

/// View current vote counts for each tree
#[command(slash_command, guild_only, ephemeral)]
pub async fn votes(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = match ctx.data().dbs.lorax.get_event(guild_id).await {
        Some(event) => event,
        None => {
            ctx.say("⚪ No active Lorax event is running.").await?;
            return Ok(());
        }
    };

    let has_manage_messages = ctx.author_member().await.map_or(false, |m| {
        m.permissions(ctx.serenity_context())
            .map_or(false, |p| p.manage_messages())
    });

    if !matches!(event.stage, LoraxStage::Completed) && !has_manage_messages {
        ctx.say("❌ Votes can only be viewed after the event is completed.")
            .await?;
        return Ok(());
    }

    let total_votes = event.tree_votes.len();
    
    // Count votes per tree
    let mut vote_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for tree in event.tree_votes.values() {
        *vote_counts.entry(tree.clone()).or_insert(0) += 1;
    }

    // Convert to vec for sorting
    let mut vote_counts: Vec<_> = vote_counts.into_iter().collect();
    vote_counts.sort_by(|a, b| b.1.cmp(&a.1));

    let msg = if vote_counts.is_empty() {
        "📝 No votes cast yet!".to_string()
    } else {
        let vote_lines: Vec<String> = vote_counts
            .iter()
            .map(|(tree, count)| {
                let percentage = if total_votes > 0 {
                    (*count as f64 / total_votes as f64) * 100.0
                } else {
                    0.0
                };
                format!("\"{}\" - {} votes ({:.1}%)", tree, count, percentage)
            })
            .collect();

        format!(
            "🗳️ **Current Vote Counts ({})**:\n{}",
            total_votes,
            vote_lines.join("\n")
        )
    };

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
