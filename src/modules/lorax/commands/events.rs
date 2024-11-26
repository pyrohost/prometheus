//! Commands for managing Lorax events.

use std::sync::Arc;

use crate::modules::lorax::{database::LoraxStage, task::LoraxEventTask};
use crate::{Context, Error};
use poise::command;
use poise::serenity_prelude::{self as serenity, ChannelId, EditMessage, Mentionable};

/// Kick off a new Lorax event for your community!
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn start(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    if let Some(event) = ctx.data().dbs.lorax.get_event(guild_id).await {
        if event.stage != LoraxStage::Inactive {
            ctx.say("❌ There is already an active event!").await?;
            return Ok(());
        }
    }

    let settings = ctx.data().dbs.lorax.get_settings(guild_id).await?;

    if settings.lorax_channel.is_none() {
        ctx.say("❌ Please set a Lorax channel first using `/lorax config channel`")
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

/// Check the current status of the Lorax event
#[command(slash_command, guild_only)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active Lorax event")?;

    let medal_emojis = ["🥇", "🥈", "🥉"];

    let status_msg = match &event.stage {
        LoraxStage::Submission => format!(
            "📝 Submission phase\nSubmissions: {}",
            event.tree_submissions.len()
        ),
        stage @ (LoraxStage::Voting | LoraxStage::Tiebreaker(_)) => {
            let mut msg = if let LoraxStage::Tiebreaker(round) = stage {
                format!("🎯 Tiebreaker Round {}\n", round)
            } else {
                "🗳️ Voting phase\n".to_string()
            };

            for (i, tree) in event.current_trees.iter().take(3).enumerate() {
                let medal = medal_emojis.get(i).unwrap_or(&"").to_string();
                if let Some(submitter_id) = event.get_tree_submitter(tree) {
                    msg.push_str(&format!("{} {} (by <@{}>)\n", medal, tree, submitter_id));
                } else {
                    msg.push_str(&format!("{} {}\n", medal, tree));
                }
            }

            if event.current_trees.len() > 3 {
                msg.push_str(&format!(
                    "... and {} other entries\n",
                    event.current_trees.len() - 3
                ));
            }

            msg.push_str(&format!("\nVotes: {}", event.tree_votes.len()));
            msg
        }
        LoraxStage::Completed => {
            let mut msg = format!(
                "✨ Event completed\nWinner: {}",
                event
                    .current_trees
                    .first()
                    .unwrap_or(&"Unknown".to_string())
            );

            msg.push_str("\n\n🏆 Final Results:");
            for (i, tree) in event.current_trees.iter().enumerate() {
                let medal = medal_emojis.get(i).unwrap_or(&"").to_string();
                if let Some(submitter_id) = event.get_tree_submitter(tree) {
                    msg.push_str(&format!("\n{} {} (by <@{}>)", medal, tree, submitter_id));
                } else {
                    msg.push_str(&format!("\n{} {}", medal, tree));
                }
            }

            if event.current_trees.len() > 3 {
                msg.push_str(&format!(
                    "\n\n... and {} other runner-up{}!",
                    event.current_trees.len() - 3,
                    if event.current_trees.len() - 3 == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
            msg
        }
        LoraxStage::Inactive => "⚪ No active event".to_string(),
    };

    ctx.say(format!("📢 **Current Lorax Status:**\n{}", status_msg))
        .await?;
    Ok(())
}

/// Skip to the next event stage
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn force_advance(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active Lorax event")?;

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
        let _ = ctx
            .data()
            .dbs
            .lorax
            .update_event(guild_id, updated_event)
            .await;
    }

    ctx.say("⏩ Advanced to the next stage!").await?;
    Ok(())
}

/// Get detailed stats about the Lorax event
#[command(slash_command, guild_only)]
pub async fn stats(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active Lorax event")?;

    let lorax_task = LoraxEventTask::new(guild_id, Arc::new(ctx.data().dbs.lorax.clone()));
    let duration = lorax_task.calculate_stage_duration(&event);
    let end_time = event.start_time + duration;

    let medal_emojis = ["🥇", "🥈", "🥉"];

    let stage_info = match &event.stage {
        LoraxStage::Submission => format!(
            "🌿 **Submission Phase**\n\
            Submissions: {}\n\
            Most Recent: {}",
            event.tree_submissions.len(),
            event
                .tree_submissions
                .iter()
                .last()
                .map(|(id, name)| format!("\"{}\" (by <@{}>)", name, id))
                .unwrap_or_else(|| "None".to_string())
        ),
        stage @ (LoraxStage::Voting | LoraxStage::Tiebreaker(_)) => {
            let mut msg = if let LoraxStage::Tiebreaker(round) = stage {
                format!("🎯 **Tiebreaker Round {}**\n", round)
            } else {
                "🗳️ **Voting Phase**\n".to_string()
            };

            msg.push_str(&format!(
                "Votes Cast: {}/{} possible\n\
                Participation: {}%\n\n\
                Current Standings:",
                event.tree_votes.len(),
                event.tree_submissions.len().max(1) - 1,
                (event.tree_votes.len() as f32 / (event.tree_submissions.len().max(1) - 1) as f32
                    * 100.0) as u32
            ));

            for (i, tree) in event.current_trees.iter().take(3).enumerate() {
                let medal = medal_emojis.get(i).unwrap_or(&"").to_string();
                if let Some(submitter_id) = event.get_tree_submitter(tree) {
                    msg.push_str(&format!("\n{} {} (by <@{}>)", medal, tree, submitter_id));
                } else {
                    msg.push_str(&format!("\n{} {}", medal, tree));
                }
            }

            if event.current_trees.len() > 3 {
                msg.push_str(&format!(
                    "\n\n... and {} other entries in the running!",
                    event.current_trees.len() - 3
                ));
            }
            msg
        }
        LoraxStage::Completed => {
            let mut msg = format!(
                "✨ **Event Completed**\n\
                Total Submissions: {}\n\
                Final Vote Count: {}\n\n\
                ���� Final Results:",
                event.tree_submissions.len(),
                event.tree_votes.len()
            );

            for (i, tree) in event.current_trees.iter().enumerate() {
                let medal = medal_emojis.get(i).unwrap_or(&"").to_string();
                if let Some(submitter_id) = event.get_tree_submitter(tree) {
                    msg.push_str(&format!("\n{} {} (by <@{}>)", medal, tree, submitter_id));
                } else {
                    msg.push_str(&format!("\n{} {}", medal, tree));
                }
            }

            if event.current_trees.len() > 3 {
                msg.push_str(&format!(
                    "\n\n... and {} other runner-up{}!",
                    event.current_trees.len() - 3,
                    if event.current_trees.len() - 3 == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
            msg
        }
        LoraxStage::Inactive => "⚪ Event is inactive".to_string(),
    };

    let timing = if matches!(event.stage, LoraxStage::Completed | LoraxStage::Inactive) {
        "Event has ended".to_string()
    } else {
        format!("Stage Ends: <t:{}:R>", end_time)
    };

    let msg = format!(
        "📊 **Lorax Event Statistics**\n\n\
        {}\n\n\
        ⏰ **Timing**\n\
        Started: <t:{}:F>\n\
        {}",
        stage_info, event.start_time, timing
    );

    ctx.say(msg).await?;
    Ok(())
}

/// Adjust the current stage duration
#[command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn duration(
    ctx: Context<'_>,
    #[description = "Minutes to add or remove (negative to reduce)"] minutes: i64,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let mut event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active Lorax event")?;

    let lorax_task = LoraxEventTask::new(guild_id, Arc::new(ctx.data().dbs.lorax.clone()));

    let current_duration = lorax_task.calculate_stage_duration(&event);
    let new_duration = (current_duration as i64 + (minutes * 60)) as u64;

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    event.start_time = current_time - (current_duration - new_duration);

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

    ctx.data()
        .dbs
        .lorax
        .write(|db| {
            db.events.remove(&guild_id);
            db.settings.remove(&guild_id);
            Ok(())
        })
        .await?;

    ctx.say("🔄 Lorax has been reset for this server.").await?;
    Ok(())
}

/// View all submissions and who submitted them
#[command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "MANAGE_MESSAGES"
)]
pub async fn submissions(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active event")?;

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

/// View current votes and who voted for what
#[command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "MANAGE_MESSAGES"
)]
pub async fn votes(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active event")?;

    if !matches!(event.stage, LoraxStage::Voting | LoraxStage::Tiebreaker(_)) {
        ctx.say("❌ Voting is not currently active.").await?;
        return Ok(());
    }

    let mut votes: Vec<_> = event
        .tree_votes
        .iter()
        .map(|(user_id, tree)| format!("<@{}> voted for \"{}\"", user_id, tree))
        .collect();
    votes.sort();

    let msg = if votes.is_empty() {
        "📝 No votes cast yet!".to_string()
    } else {
        format!(
            "🗳️ **Current Votes ({})**:\n{}",
            votes.len(),
            votes.join("\n")
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

    let mut event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active event")?;

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

    let mut event = ctx
        .data()
        .dbs
        .lorax
        .get_event(guild_id)
        .await
        .ok_or("No active event")?;

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
