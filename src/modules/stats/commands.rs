use super::database::{DataType, GuildSettings, StatBar};
use super::task::StatsTask;
use crate::{Context, Error};
use poise::command;
use poise::serenity_prelude::{builder::CreateChannel, ChannelId, ChannelType};

#[command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn set_prometheus(
    ctx: Context<'_>,
    #[description = "Prometheus server URL"] url: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    StatsTask::query_prometheus(&url, "up").await?;

    ctx.data()
        .dbs
        .stats
        .transaction(|db| {
            let mut settings = GuildSettings::default();
            settings.prometheus_url = url;
            db.guild_settings.insert(guild_id, settings);
            Ok(())
        })
        .await?;

    ctx.say("✅ Prometheus server URL set!").await?;
    Ok(())
}

/// Set a stat bar for a voice channel
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn set(
    ctx: Context<'_>,
    #[description = "Voice channel to use"] channel: ChannelId,
    #[description = "Prometheus query"] query: String,
    #[description = "Display format (use {value} for the value)"] format: String,
    #[description = "Value type"] data_type: DataType,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let channel_info = channel.to_channel(&ctx.serenity_context()).await?;
    if !matches!(channel_info.guild(), Some(c) if c.kind == ChannelType::Voice) {
        ctx.say("❌ Please select a voice channel!").await?;
        return Ok(());
    }

    let prometheus_url = ctx
        .data()
        .dbs
        .stats
        .get_settings(guild_id)
        .await?
        .prometheus_url;
    if prometheus_url.is_empty() {
        ctx.say("❌ Please set a Prometheus server URL first using `/stats set_prometheus`!")
            .await?;
        return Ok(());
    }

    let _test_value = StatsTask::query_prometheus(&prometheus_url, &query).await?;

    let stat_bar = StatBar {
        channel_id: channel.get(),
        query,
        format,
        data_type,
        last_value: None,
        last_update: None,
    };

    ctx.data()
        .dbs
        .stats
        .update_stat_bar(guild_id, stat_bar)
        .await?;
    ctx.say("✅ Stat bar set! The channel name will update shortly.")
        .await?;
    Ok(())
}

/// Create a new voice channel with a stat bar
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn create_channel(
    ctx: Context<'_>,
    #[description = "Name for the new channel"] name: String,
    #[description = "Prometheus query"] query: String,
    #[description = "Display format (use {value} for the value)"] format: String,
    #[description = "Value type"] data_type: DataType,
    #[description = "Optional category to create the channel in"] category: Option<ChannelId>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let prometheus_url = ctx
        .data()
        .dbs
        .stats
        .get_settings(guild_id.get())
        .await?
        .prometheus_url;
    if prometheus_url.is_empty() {
        ctx.say("❌ Please set a Prometheus server URL first using `/stats set_prometheus`!")
            .await?;
        return Ok(());
    }

    let test_value = StatsTask::query_prometheus(&prometheus_url, &query).await?;

    let mut channel_builder = CreateChannel::new(name).kind(ChannelType::Voice);

    if let Some(cat_id) = category {
        channel_builder = channel_builder.category(cat_id);
    }

    let channel = guild_id
        .create_channel(&ctx.serenity_context(), channel_builder)
        .await?;

    let stat_bar = StatBar {
        channel_id: channel.id.get(),
        query,
        format,
        data_type,
        last_value: Some(test_value),
        last_update: Some(std::time::SystemTime::now()),
    };

    ctx.data()
        .dbs
        .stats
        .update_stat_bar(guild_id.get(), stat_bar)
        .await?;
    ctx.say(format!(
        "✅ Created voice channel with stat bar! <#{}>",
        channel.id
    ))
    .await?;
    Ok(())
}

/// Remove a stat bar from a voice channel
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Voice channel to remove stats from"] channel: ChannelId,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let removed = ctx
        .data()
        .dbs
        .stats
        .transaction(|db| {
            if let Some(bars) = db.stat_bars.get_mut(&guild_id) {
                Ok(bars.remove(&channel.get()).is_some())
            } else {
                Ok(false)
            }
        })
        .await?;

    if removed {
        ctx.say("✅ Stat bar removed!").await?;
    } else {
        ctx.say("❌ No stat bar found for this channel.").await?;
    }

    Ok(())
}

/// List all stat bars in the server
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let stat_bars = ctx
        .data()
        .dbs
        .stats
        .read(|db| {
            db.stat_bars
                .get(&guild_id)
                .map(|bars| bars.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .await;

    if stat_bars.is_empty() {
        ctx.say("No stat bars configured.").await?;
        return Ok(());
    }

    let mut response = String::from("📊 **Stat Bars**\n");
    for bar in &stat_bars {
        response.push_str(&format!(
            "• <#{}>\n  Query: `{}`\n  Format: `{}`\n  Type: `{:?}`\n",
            bar.channel_id, bar.query, bar.format, bar.data_type
        ));
    }

    ctx.say(response).await?;
    Ok(())
}

/// Show the current Prometheus server URL
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn show_prometheus(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();

    let url = ctx
        .data()
        .dbs
        .stats
        .read(|db| {
            db.guild_settings
                .get(&guild_id)
                .map(|s| s.prometheus_url.clone())
        })
        .await;

    match url {
        Some(url) => {
            ctx.say(format!("🔗 Current Prometheus URL: `{}`", url))
                .await?
        }
        None => ctx.say("❌ No Prometheus URL configured!").await?,
    };

    Ok(())
}

/// Set how often stat bars should update (in seconds)
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_CHANNELS")]
pub async fn set_delay(
    ctx: Context<'_>,
    #[description = "Update delay in seconds (minimum 30)"] delay: u64,
) -> Result<(), Error> {
    if delay < 30 {
        ctx.say("❌ Minimum delay is 30 seconds!").await?;
        return Ok(());
    }

    let guild_id = ctx.guild_id().unwrap().get();

    ctx.data()
        .dbs
        .stats
        .transaction(|db| {
            let settings = db.guild_settings.entry(guild_id).or_default();
            settings.update_delay = delay;
            Ok(())
        })
        .await?;

    ctx.say(format!(
        "✅ Stat bars will now update every {} seconds!",
        delay
    ))
    .await?;
    Ok(())
}

#[command(
    slash_command,
    subcommands(
        "set_prometheus",
        "show_prometheus",
        "set_delay",
        "set",
        "create_channel",
        "remove",
        "list"
    )
)]
pub async fn stats(_ctx: crate::Context<'_>) -> Result<(), crate::Error> {
    Ok(())
}
