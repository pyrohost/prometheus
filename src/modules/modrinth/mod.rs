pub mod commands;
pub mod database;

use commands::*;
use poise::command;

/// 🔗 Link your Modrinth account
#[command(
    slash_command,
    subcommands("link", "unlink"),
    guild_only,
    category = "Account"
)]
pub async fn modrinth(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}
