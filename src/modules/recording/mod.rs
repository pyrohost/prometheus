pub mod commands;
pub mod database;
pub mod handler;

use commands::*;
use poise::command;

/// 🎙️ Voice channel recording
#[command(
    slash_command,
    subcommands("enable", "disable", "list", "toggle"),
    guild_only,
    required_permissions = "MANAGE_GUILD"
)]
pub async fn recording(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}
