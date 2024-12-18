pub mod commands;
pub mod database;
pub mod task;

use commands::*;
use poise::command;

/// 🧪 Create and manage temporary Minecraft test servers
#[command(
    slash_command,
    subcommands("create", "delete", "list", "extend"),
    guild_only,
    category = "Servers"
)]
pub async fn servers(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}

pub use servers as testing;
