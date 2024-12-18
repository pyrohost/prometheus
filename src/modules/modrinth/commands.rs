use crate::{Context, Error};
use poise::command;
use serde_json::Value;

const VERIFICATION_CODE: &str = "PYRO-";

/// Start linking your Modrinth account
#[command(slash_command, guild_only, ephemeral)]
pub async fn link(ctx: Context<'_>) -> Result<(), Error> {
    let discord_id = ctx.author().id.get();

    if let Some(_) = ctx.data().dbs.modrinth.get_modrinth_id(discord_id).await {
        ctx.say("⚠️ Your account is already linked! Use `/modrinth unlink` first.")
            .await?;
        return Ok(());
    }

    let verification_code = format!("{}{}", VERIFICATION_CODE, discord_id);

    ctx.say(format!(
        "🔗 **Link your Modrinth Account**\n\n\
        1. Visit your [Modrinth profile settings](https://modrinth.com/settings/account)\n\
        2. Add this code to your bio: `{}`\n\
        3. Use `/modrinth verify` to complete linking\n\n\
        Note: You can remove the code from your bio after verification.",
        verification_code
    ))
    .await?;

    Ok(())
}

/// Complete Modrinth account verification
#[command(slash_command, guild_only, ephemeral)]
pub async fn verify(ctx: Context<'_>) -> Result<(), Error> {
    let discord_id = ctx.author().id.get();
    let verification_code = format!("{}{}", VERIFICATION_CODE, discord_id);

    let client = reqwest::Client::new();
    let username = ctx.author().name.clone();
    let response: Value = client
        .get(format!("https://api.modrinth.com/v2/user/{}", username))
        .send()
        .await?
        .json()
        .await?;

    let bio = response["bio"].as_str().unwrap_or("");
    if !bio.contains(&verification_code) {
        ctx.say("❌ Verification code not found in your Modrinth profile bio! Please add it and try again.").await?;
        return Ok(());
    }

    let modrinth_id = response["id"].as_str().unwrap_or("").to_string();
    ctx.data()
        .dbs
        .modrinth
        .link_account(discord_id, modrinth_id)
        .await?;

    ctx.say("✅ Successfully linked your Modrinth account! You can now remove the verification code from your bio.").await?;
    Ok(())
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
