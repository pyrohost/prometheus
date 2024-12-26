use super::database::TestServer;
use crate::{Context, Error};
use poise::serenity_prelude::{self as serenity, ButtonStyle, CreateActionRow, CreateButton};
use poise::{command, CreateReply};
use serde_json::{json, Value};
use std::time::{Duration, SystemTime};
use reqwest::Client;
use tracing::error;

const MAX_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

async fn format_expiry(time: SystemTime) -> String {
    let expires = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("<t:{}:R>", expires)
}

async fn send_api_request(
    ctx: Context<'_>,
    url: &str,
    method: reqwest::Method,
    payload: Option<Value>,
) -> Result<Value, Error> {
    let client = Client::new();
    let mut request = client
        .request(method, url)
        .header("X-MASTER-KEY", &ctx.data().config.master_key);

    if let Some(payload) = payload {
        request = request.json(&payload);
    }

    let response = request.send().await?;
    let response: Value = response.json().await?;
    Ok(response)
}

async fn check_administrator(ctx: &Context<'_>) -> bool {
    let Some(member) = ctx.author_member().await else { return false };
    let Some(_guild) = ctx.guild() else { return false };

    member.permissions.map_or(false, |p| p.administrator())
}

/// Create a temporary test server for Minecraft development
/// 
/// Creates a server with specified resources that will automatically be deleted after expiry.
/// Regular staff get 1GB RAM servers, while administrators can configure custom specs.
#[command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS", ephemeral)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Server name (defaults to your username)"] name: Option<String>,
    #[description = "Lifetime in hours (admins: unlimited, others: max 24)"] hours: Option<u64>,
    #[description = "Create for another user (admin only)"] user: Option<serenity::User>,
    #[description = "Create for specific Modrinth ID (admin only)"] modrinth_id: Option<String>,
    #[description = "RAM in GB (admin only)"] ram_gb: Option<f32>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let is_admin = check_administrator(&ctx).await;

    // Ensure only admins can use user/modrinth_id parameters
    if (user.is_some() || modrinth_id.is_some()) && !is_admin {
        ctx.say("❌ Administrator permission required to create servers for others!").await?;
        return Ok(());
    }

    // Ensure only one of user or modrinth_id is specified
    if user.is_some() && modrinth_id.is_some() {
        ctx.say("❌ Cannot specify both user and Modrinth ID!").await?;
        return Ok(());
    }

    let ram_gb = if is_admin {
        ram_gb.unwrap_or(2.0)
    } else {
        if ram_gb.is_some() {
            ctx.say("❌ Only administrators can configure server RAM!").await?;
            return Ok(());
        }
        1.0
    };

    // Resolve user ID and Modrinth ID
    let (user_id, modrinth_id) = if let Some(ref target_user) = user {
        let user_id = target_user.id.get();
        match ctx.data().dbs.modrinth.get_modrinth_id(user_id).await {
            Some(id) => (user_id, id),
            None => {
                ctx.say("❌ Target user has not linked their Modrinth account!").await?;
                return Ok(());
            }
        }
    } else if let Some(mid) = modrinth_id {
        // When using direct Modrinth ID, use the admin's user ID
        (ctx.author().id.get(), mid)
    } else {
        // Default case - use command author
        let user_id = ctx.author().id.get();
        match ctx.data().dbs.modrinth.get_modrinth_id(user_id).await {
            Some(id) => (user_id, id),
            None => {
                ctx.say("❌ Please link your Modrinth account first:\n> Use `/modrinth link` to get started").await?;
                return Ok(());
            }
        }
    };

    let current_servers = ctx.data().dbs.testing.get_user_servers(user_id).await;
    let user_limit = ctx.data().dbs.testing.get_user_limit(user_id).await;

    if current_servers.len() >= user_limit {
        ctx.say(format!(
            "❌ User has reached their server limit ({}/{})",
            current_servers.len(), user_limit
        )).await?;
        return Ok(());
    }

    let username = if let Some(u) = user {
        u.name.clone()
    } else {
        ctx.author().name.clone()
    };

    let server_name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("{}'s Test Server", username));

    let duration = Duration::from_secs(hours.unwrap_or(8) * 3600);
    if !is_admin && duration > MAX_DURATION {
        ctx.say("❌ Maximum server duration is 24 hours for non-administrator users!").await?;
        return Ok(());
    }

    ctx.defer().await?;

    let base_ram = (ram_gb * 1024.0) as u32;
    let payload = json!({
        "user_id": modrinth_id,
        "name": server_name,
        "testing": true,
        "specs": {
            "cpu": ((base_ram as f32 / 2048.0).ceil() as u32).max(2), // Minimum 2 CPUs, no max
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

    let response = send_api_request(
        ctx.clone(),
        "https://archon.pyro.host/modrinth/v0/servers/create",
        reqwest::Method::POST,
        Some(payload),
    ).await?;

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

    let expires_at = server.expires_at;
    ctx.data().dbs.testing.add_server(server).await?;

    let expiry_str = format_expiry(expires_at).await;

    ctx.say(format!(
        "✅ Created test server successfully!\n> **{}**\n> Expires {}\n> Manage at: https://modrinth.com/servers/manage/{}",
        server_name,
        expiry_str,
        server_id
    )).await?;

    Ok(())
}

/// Set the maximum number of test servers a user can create
/// 
/// Administrators can grant users the ability to create multiple test servers simultaneously.
/// The default limit is 1 server per user.
#[command(
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn setlimit(
    ctx: Context<'_>,
    #[description = "User to modify limit for"] user: serenity::User,
    #[description = "New server limit (default: 1)"]
    #[min = 1]
    #[max = 10]
    limit: Option<usize>,
) -> Result<(), Error> {
    let limit = limit.unwrap_or(1);
    ctx.data().dbs.testing.set_user_limit(user.id.get(), limit).await?;

    ctx.say(format!(
        "✅ Set {}'s server limit to {}",
        user.name, limit
    )).await?;
    Ok(())
}

/// View all users with custom server limits
/// 
/// Shows a list of users who have been granted permission to create multiple test servers.
/// Users not listed have the default limit of 1 server.
#[command(
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn limits(ctx: Context<'_>) -> Result<(), Error> {
    let limits = ctx.data().dbs.testing
        .read(|db| db.user_limits.clone())
        .await;

    if limits.is_empty() {
        ctx.say("📊 No custom server limits set.").await?;
        return Ok(());
    }

    let mut response = String::from("📊 **Custom Server Limits**\n");
    for (user_id, limit) in limits {
        response.push_str(&format!("• <@{}> - {} servers\n", user_id, limit));
    }

    ctx.say(response).await?;
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

    let usernames: Vec<String> = servers
        .iter()
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
                server.server_id,
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
}

/// Delete test servers
/// 
/// Removes one or more test servers immediately. Administrators can delete any server,
/// while regular users can only delete their own servers.
#[command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    ephemeral
)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "Specific server to delete (admins only)"]
    #[autocomplete = "autocomplete_server_id"]
    server_id: Option<String>,
    #[description = "Delete all of your servers"] 
    all: Option<bool>,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let is_admin = check_administrator(&ctx).await;
    let user_id = ctx.author().id.get();

    let servers = if let Some(server_id) = server_id {
        // Admin deleting specific server
        if !is_admin {
            ctx.say("❌ Administrator permission required to delete specific servers!")
                .await?;
            return Ok(());
        }

        if let Some(server) = ctx.data()
            .dbs
            .testing
            .read(|db| db.servers.get(&server_id).cloned())
            .await
        {
            vec![server]
        } else {
            ctx.say("❌ Server not found!").await?;
            return Ok(());
        }
    } else if all.unwrap_or(false) {
        // Deleting all user's servers
        let servers = ctx.data().dbs.testing.get_user_servers(user_id).await;
        if servers.is_empty() {
            ctx.say("❌ You don't have any active servers!").await?;
            return Ok(());
        }
        servers
    } else {
        // Deleting single user server
        if let Some(server) = ctx.data().dbs.testing.get_user_server(user_id).await {
            vec![server]
        } else {
            ctx.say("❌ You don't have an active server!").await?;
            return Ok(());
        }
    };

    let count = servers.len();
    let multiple = count > 1;

    let confirmation = format!(
        "🗑️ Are you sure you want to delete {} test {}?\n{}",
        if multiple { format!("these {} ", count) } else { "this".into() },
        if multiple { "servers" } else { "server" },
        servers.iter().map(|s| format!(
            "> **{}**\n> Created <t:{}:R>{}",
            s.name,
            s.created_at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            if is_admin && s.user_id != user_id {
                format!("\n> Owner: <@{}>", s.user_id)
            } else {
                String::new()
            }
        )).collect::<Vec<_>>().join("\n\n")
    );

    let button = CreateButton::new("confirm")
        .style(ButtonStyle::Danger)
        .label(format!("Delete {}", if multiple { "Servers" } else { "Server" }));

    let action_row = CreateActionRow::Buttons(vec![button]);
    let reply = CreateReply::default()
        .content(confirmation)
        .components(vec![action_row]);

    let confirm = ctx.send(reply).await?;
    let interaction = confirm
        .message()
        .await?
        .await_component_interaction(ctx.serenity_context())
        .author_id(ctx.author().id)
        .timeout(Duration::from_secs(30))
        .await;

    let Some(interaction) = interaction else {
        confirm.edit(ctx, CreateReply::default()
            .content("❌ Operation timed out")
            .components(vec![]))
            .await?;
        return Ok(());
    };

    interaction.defer_ephemeral(ctx.serenity_context()).await?;

    // Update message to show deletion in progress
    confirm.edit(ctx, CreateReply::default()
        .content("🔄 Deleting server(s)...")
        .components(vec![]))
        .await?;

    let client = reqwest::Client::new();
    let mut deleted = 0;

    for server in &servers {
        match client
            .post(format!(
                "https://archon.pyro.host/modrinth/v0/servers/{}/delete",
                server.server_id
            ))
            .header("X-MASTER-KEY", &ctx.data().config.master_key)
            .send()
            .await
        {
            Ok(_) => {
                if let Err(e) = ctx.data()
                    .dbs
                    .testing
                    .remove_server(&server.server_id)
                    .await
                {
                    error!("Failed to remove server from database: {}", e);
                } else {
                    deleted += 1;
                }
            }
            Err(e) => error!("Failed to delete server {}: {}", server.server_id, e),
        }
    }

    // Show final status after deletion is complete
    let status = if deleted == count {
        format!("✅ Successfully deleted {} {}!", 
            if multiple { format!("all {}", count) } else { "the".into() },
            if multiple { "servers" } else { "server" }
        )
    } else {
        format!("⚠️ Partially deleted servers ({}/{})", deleted, count)
    };

    confirm.edit(ctx, CreateReply::default()
        .content(status)
        .components(vec![]))
        .await?;

    Ok(())
}

/// List all active test servers
/// 
/// Shows all currently running test servers, their owners, creation times,
/// and expiration times.
#[command(
    slash_command,
    guild_only,
    ephemeral,
    required_permissions = "MANAGE_CHANNELS"
)]
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

/// Extend a test server's lifetime
/// 
/// Adds more time before the server is automatically deleted.
/// Regular users are limited to 24h extensions, while administrators can extend indefinitely.
#[command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_CHANNELS",
    ephemeral
)]
pub async fn extend(
    ctx: Context<'_>,
    #[description = "Additional hours (admins: unlimited, others: max 24)"]
    hours: u64,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let is_admin = check_administrator(&ctx).await;
    let duration = Duration::from_secs(hours * 3600);
    
    if !is_admin && duration > MAX_DURATION {
        ctx.say("❌ Maximum extension is 24 hours for non-administrator users!").await?;
        return Ok(());
    }

    let user_id = ctx.author().id.get();

    let server = match ctx.data().dbs.testing.get_user_server(user_id).await {
        Some(s) => s,
        None => {
            ctx.say("❌ You don't have a test server!").await?;
            return Ok(());
        }
    };

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
