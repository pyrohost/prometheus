pub mod commands;
pub mod database;

use commands::*;
use poise::command;

/// 🔗 Link your Modrinth account
#[command(
    slash_command,
    subcommands("link", "unlink", "verify"),
    guild_only,
    category = "Account"
)]
pub async fn account(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}

pub use account as modrinth;
