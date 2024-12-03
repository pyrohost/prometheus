pub mod commands;
pub mod database;
pub mod task;

use commands::*;
use poise::command;

/// 📊 Prometheus stat bars in voice channels
#[command(
    slash_command,
    subcommands(
        "set_prometheus",
        "show_prometheus",
        "set",
        "create_channel",
        "remove",
        "list"
    )
)]
pub async fn stats(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}
