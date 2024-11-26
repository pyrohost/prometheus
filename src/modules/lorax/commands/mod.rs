use poise::command;

pub mod events;
pub mod settings;
pub mod users;

/// 🌳 Tree name voting system for your server
#[command(
    slash_command,
    subcommands(
        "events::start",
        "events::end",
        "events::status",
        "events::stats",
        "events::duration",
        "events::force_advance",
        "events::reset",
        "events::submissions",
        "events::votes",
        "events::remove_submission",
        "events::remove_vote",
        "settings::channel",
        "settings::roles",
        "settings::durations",
        "settings::view",
        "users::submit",
        "users::vote",
    )
)]
pub async fn lorax(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}
