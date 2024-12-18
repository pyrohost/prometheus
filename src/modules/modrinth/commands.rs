use crate::{Context, Error};
use poise::command;
use poise::serenity_prelude::{ButtonStyle, CreateActionRow, CreateButton};
use poise::CreateReply;
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

const VERIFICATION_CODE: &str = "PYRO-";
const CHECK_INTERVAL: Duration = Duration::from_secs(10);
const MAX_DURATION: Duration = Duration::from_secs(300); // 5 minutes

/// Link your Modrinth account
#[command(slash_command, guild_only, ephemeral)]
pub async fn link(ctx: Context<'_>) -> Result<(), Error> {
    let discord_id = ctx.author().id.get();

    if let Some(_) = ctx.data().dbs.modrinth.get_modrinth_id(discord_id).await {
        ctx.say("⚠️ Your account is already linked! Use `/modrinth unlink` first.")
            .await?;
        return Ok(());
    }

    let verification_code = format!("{}{}", VERIFICATION_CODE, discord_id);

    let button = CreateButton::new("retry")
        .style(ButtonStyle::Primary)
        .label("Check Now");

    let action_row = CreateActionRow::Buttons(vec![button]);
    
    let reply = CreateReply::default()
        .content(format!(
            "🔗 **Link your Modrinth Account**\n\n\
            1. Visit your [Modrinth profile settings](https://modrinth.com/settings/profile)\n\
            2. Add this code to your bio: `{}`\n\
            Checking automatically every 10 seconds...\n\n\
            Note: You can remove the code from your bio after verification.",
            verification_code
        ))
        .components(vec![action_row]);

    let msg = ctx.send(reply).await?;

    let start_time = std::time::Instant::now();
    
    loop {
        if start_time.elapsed() > MAX_DURATION {
            let edit = CreateReply::default()
                .content("❌ Verification timed out after 5 minutes. Please try again with `/modrinth link`.")
                .components(vec![]);
            msg.edit(ctx, edit).await?;
            return Ok(());
        }

        // Check for button press
        let interaction = msg
            .message()
            .await?
            .await_component_interaction(ctx.serenity_context())
            .timeout(CHECK_INTERVAL)
            .await;

        // Verify regardless of button press
        if let Ok(_) = verify_code(&ctx, &verification_code).await {
            let edit = CreateReply::default()
                .content("✅ Successfully linked your Modrinth account! You can now remove the verification code from your bio.")
                .components(vec![]);
            msg.edit(ctx, edit).await?;
            return Ok(());
        }

        // Acknowledge button press if it happened
        if let Some(interaction) = interaction {
            interaction.defer(ctx.serenity_context()).await?;
        }
    }
}

async fn verify_code(ctx: &Context<'_>, verification_code: &str) -> Result<(), Error> {
    let discord_id = ctx.author().id.get();
    let client = reqwest::Client::new();

    // Try each username variant
    for username in &[&ctx.author().name] {
        let response = client
            .get(format!("https://api.modrinth.com/v2/user/{}", username))
            .send()
            .await;

        let response = match response {
            Ok(resp) if resp.status().is_success() => resp,
            _ => continue,
        };

        let json: Value = match response.json().await {
            Ok(json) => json,
            _ => continue,
        };

        let bio = json["bio"].as_str().unwrap_or("");
        if !bio.contains(verification_code) {
            continue;
        }

        let modrinth_id = match json["id"].as_str() {
            Some(id) => id.to_string(),
            None => continue,
        };

        ctx.data()
            .dbs
            .modrinth
            .link_account(discord_id, modrinth_id)
            .await?;

        return Ok(());
    }

    Err("Verification failed".into())
}

/// Unlink your Modrinth account
#[command(slash_command, guild_only, ephemeral)]
pub async fn unlink(ctx: Context<'_>) -> Result<(), Error> {
    let discord_id = ctx.author().id.get();

    if ctx
        .data()
        .dbs
        .modrinth
        .get_modrinth_id(discord_id)
        .await
        .is_none()
    {
        ctx.say("❌ Your account is not linked!").await?;
        return Ok(());
    }

    ctx.data().dbs.modrinth.unlink_account(discord_id).await?;
    ctx.say("✅ Successfully unlinked your Modrinth account!")
        .await?;
    Ok(())
}
