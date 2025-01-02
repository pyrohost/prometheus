use poise::command;

pub mod admin;
pub mod settings;
pub mod users;

/// 🌳 Tree name voting system for your server
#[command(
    slash_command,
    subcommands(
        "admin::start",
        "admin::end",
        "admin::duration",
        "admin::force_advance",
        "admin::reset",
        "admin::submissions",
        "admin::votes",
        "admin::remove_submission",
        "admin::remove_vote",
        "settings::channel",
        "settings::roles",
        "settings::durations",
        "settings::view",
        "users::submit",
        "users::vote",
        "users::check",
    )
)]
pub async fn lorax(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}
