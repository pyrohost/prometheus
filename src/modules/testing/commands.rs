use super::database::TestServer;
use crate::{Context, Error};
use poise::serenity_prelude::{self as serenity, ButtonStyle, CreateActionRow, CreateButton};
use poise::{command, CreateReply};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime};

const MAX_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

/// Create a new test server
#[command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    ephemeral
)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Custom server name (defaults to your username)"]
    #[max_length = 32]
    name: Option<String>,
    #[description = "Server lifetime in hours (default: 8, max: 24)"]
    #[min = 1]
    #[max = 24]
    hours: Option<u64>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let user_id = ctx.author().id.get();

    let modrinth_id = match ctx.data().dbs.modrinth.get_modrinth_id(user_id).await {
        Some(id) => id,
        None => {
            ctx.say("❌ Please link your Modrinth account first:\n> Use `/modrinth link` to get started").await?;
            return Ok(());
        }
    };

    let username = ctx.author().name.clone();
    let server_name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("{}'s Test Server", username));

    if let Some(existing) = ctx.data().dbs.testing.get_user_server(user_id).await {
        let expires = existing
            .expires_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        ctx.say(format!(
            "❌ You already have an active test server:\n> **{}**\n> Expires <t:{}:R>\n> Manage at: https://modrinth.com/servers/manage/{}",
            existing.name,
            expires,
            existing.server_id
        )).await?;
        return Ok(());
    }

    let duration = Duration::from_secs(hours.unwrap_or(8) * 3600);
    if duration > MAX_DURATION {
        ctx.say("❌ Maximum server duration is 24 hours!").await?;
        return Ok(());
    }

    ctx.defer().await?;

    let base_ram = 2048;
    let payload = json!({
        "user_id": modrinth_id,
        "name": server_name,
        "testing": true,
        "specs": {
            "cpu": 2,
            "memory_mb": base_ram,
            "swap_mb": base_ram / 4,
            "storage_mb": base_ram * 8,
        },
        "source": {
            "loader": "Vanilla",
            "game_version": "latest",
            "loader_version": "latest"
        }
    });

    let client = reqwest::Client::new();
    let response = client
        .post("https://archon.pyro.host/modrinth/v0/servers/create")
        .header("X-MASTER-KEY", &ctx.data().config.master_key)
        .json(&payload)
        .send()
        .await?;

    let response: Value = response.json().await?;
    let server_id = response["uuid"]
        .as_str()
        .ok_or("Invalid server ID in response")?;

    let server = TestServer {
        server_id: server_id.to_string(),
        user_id,
        name: server_name.clone(),
        created_at: SystemTime::now(),
        expires_at: SystemTime::now() + duration,
    };

    let expires_at = server
        .expires_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    ctx.data().dbs.testing.add_server(server).await?;

    ctx.say(format!(
        "✅ Created test server successfully!\n> **{}**\n> Expires <t:{}:R>\n> Manage at: https://modrinth.com/servers/manage/{}",
        server_name,
        expires_at,
        server_id
    )).await?;

    Ok(())
}

/// Helper function for server ID autocomplete
async fn autocomplete_server_id<'a>(
    ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = serenity::AutocompleteChoice> {
    let servers = ctx
        .data()
        .dbs
        .testing
        .read(|db| db.servers.values().cloned().collect::<Vec<_>>())
        .await;

    // Get usernames from cache where possible
    let usernames: Vec<String> = servers.iter()
        .map(|server| {
            ctx.cache()
                .user(server.user_id)
                .map(|u| u.name.clone())
                .unwrap_or_else(|| format!("User {}", server.user_id))
        })
        .collect();

    servers
        .into_iter()
        .zip(usernames)
        .filter(move |(server, _)| {
            server.name.to_lowercase().contains(&partial.to_lowercase())
                || server.server_id.contains(partial)
        })
        .map(|(server, username)| {
            serenity::AutocompleteChoice::new(
                format!("{} (by {})", server.name, username),
                server.server_id
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Delete a test server
#[command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    ephemeral
)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "Specific server ID to delete (administrators only)"]
    #[autocomplete = "autocomplete_server_id"]
    server_id: Option<String>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let user_id = ctx.author().id.get();

    // If server_id is provided, check for admin permissions
    let server = if let Some(server_id) = server_id {
        // Check if user has administrator permission
        if !ctx
            .author_member()
            .await
            .and_then(|m| {
                ctx.guild()
                    .map(|g| g.user_permissions_in(&g.channels[&g.rules_channel_id.unwrap_or_default()], &m))
            })
            .map_or(false, |p| p.administrator())
        {
            ctx.say("❌ Administrator permission required to delete other servers!")
                .await?;
            return Ok(());
        }

        // Get the specified server directly using the ID
        ctx.data()
            .dbs
            .testing
            .read(|db| db.servers.get(&server_id).cloned())
            .await
    } else {
        // Get the user's own server
        ctx.data().dbs.testing.get_user_server(user_id).await
    };

    let server = match server {
        Some(s) => s,
        None => {
            ctx.say("❌ Server not found!").await?;
            return Ok(());
        }
    };

    let created_at = server
        .created_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let button = CreateButton::new("confirm")
        .style(ButtonStyle::Danger)
        .label("Delete Server");

    let action_row = CreateActionRow::Buttons(vec![button]);

    let owner_note = if server.user_id != user_id {
        format!("\n> Owner: <@{}>", server.user_id)
    } else {
        String::new()
    };

    let reply = CreateReply::default()
        .ephemeral(true)
        .content(format!(
            "🗑️ Are you sure you want to delete this test server?\n> **{}**\n> Created <t:{}:R>{}",
            server.name, created_at, owner_note
        ))
        .components(vec![action_row]);

    let confirm = ctx.send(reply).await?;

    let interaction = confirm
        .message()
        .await?
        .await_component_interaction(ctx.serenity_context())
        .timeout(Duration::from_secs(30))
        .await;

    if interaction.is_none() {
        let edit_reply = CreateReply::default().content("❌ Operation timed out");
        confirm.edit(ctx, edit_reply).await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;

    let client = reqwest::Client::new();
    client
        .post(format!(
            "https://archon.pyro.host/modrinth/v0/servers/{}/delete",
            server.server_id
        ))
        .header("X-MASTER-KEY", &ctx.data().config.master_key)
        .send()
        .await?;

    ctx.data()
        .dbs
        .testing
        .remove_server(&server.server_id)
        .await?;

    let edit_reply = CreateReply::default().content("✅ Test server deleted successfully!");
    confirm.edit(ctx, edit_reply).await?;

    Ok(())
}

/// List all active test servers
#[command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let servers = ctx
        .data()
        .dbs
        .testing
        .read(|db| db.servers.values().cloned().collect::<Vec<_>>())
        .await;

    if servers.is_empty() {
        ctx.say("📭 No active test servers.").await?;
        return Ok(());
    }

    let mut response = String::from("📊 **Active Test Servers**\n");
    for (i, server) in servers.iter().enumerate() {
        let expires = server
            .expires_at
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        response.push_str(&format!(
            "\n**{}**. {} (<@{}>)\n> Created <t:{}:R> • Expires <t:{}:R>\n> https://modrinth.com/servers/manage/{}\n",
            i + 1,
            server.name,
            server.user_id,
            server.created_at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            expires,
            server.server_id
        ));
    }

    ctx.say(response).await?;
    Ok(())
}

/// Extend your test server's lifetime
#[command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    ephemeral
)]
pub async fn extend(
    ctx: Context<'_>,
    #[description = "Additional hours to add (max: 24)"]
    #[min = 1]
    #[max = 24]
    hours: u64,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let user_id = ctx.author().id.get();

    let server = match ctx.data().dbs.testing.get_user_server(user_id).await {
        Some(s) => s,
        None => {
            ctx.say("❌ You don't have a test server!").await?;
            return Ok(());
        }
    };

    let duration = Duration::from_secs(hours * 3600);
    if duration > MAX_DURATION {
        ctx.say("❌ Maximum extension is 24 hours!").await?;
        return Ok(());
    }

    ctx.data()
        .dbs
        .testing
        .extend_server(&server.server_id, duration)
        .await?;

    let new_expiry = (SystemTime::now() + duration)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    ctx.say(format!(
        "✅ Extended server lifetime! New expiry: <t:{}:R>",
        new_expiry
    ))
    .await?;
    Ok(())
}
