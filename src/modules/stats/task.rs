use crate::tasks::Task;
use crate::{database::Database, modules::stats::database::StatsDatabase};
use async_trait::async_trait;
use poise::serenity_prelude::{ChannelId, Context, EditChannel};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

use super::database::StatBar;

#[derive(Debug)]
pub struct StatsTask {
    db: Database<StatsDatabase>,
}

impl StatsTask {
    pub fn new(db: Database<StatsDatabase>) -> Self {
        Self { db }
    }

    pub async fn query_prometheus(
        url: &str,
        query: &str,
    ) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Querying Prometheus - {}", query);
        let start = std::time::Instant::now();

        #[derive(serde::Deserialize)]
        struct PrometheusResponse {
            data: Data,
        }

        #[derive(serde::Deserialize)]
        struct Data {
            result: Vec<Result>,
        }

        #[derive(serde::Deserialize)]
        struct Result {
            value: (i64, String),
        }

        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/api/v1/query", url))
            .query(&[("query", query)])
            .send()
            .await?;

        debug!("Query time: {:?}", start.elapsed());

        let response = response.json::<PrometheusResponse>().await?;

        if let Some(first_result) = response.data.result.first() {
            let value = first_result.value.1.parse::<f64>()?;
            debug!("Got value {} for {}", value, query);
            Ok(value)
        } else {
            error!("Empty response for query {}", query);
            Err("No data returned from Prometheus".into())
        }
    }

    async fn update_stat_bar(
        ctx: &Context,
        prometheus_url: &str,
        stat_bar: &mut StatBar,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let value = Self::query_prometheus(prometheus_url, &stat_bar.query).await?;
        let channel = ChannelId::new(stat_bar.channel_id);
        let formatted_value = stat_bar.data_type.format_value(value);
        let new_name = stat_bar.format.replace("{value}", &formatted_value);

        let channel_info =
            match timeout(Duration::from_secs(5), channel.to_channel(&ctx.http)).await {
                Ok(Ok(info)) => info,
                Ok(Err(e)) => {
                    warn!("Failed to fetch channel {}: {}", stat_bar.channel_id, e);
                    return Ok(());
                }
                Err(_) => {
                    warn!("Timeout fetching channel {}", stat_bar.channel_id);
                    return Ok(());
                }
            };

        if let Some(current_name) = channel_info.guild().map(|c| c.name().to_string()) {
            if current_name == new_name {
                stat_bar.last_value = Some(value);
                debug!(
                    "Skipping update for {} - value unchanged",
                    stat_bar.channel_id
                );
                return Ok(());
            }

            if let Some(prev_value) = stat_bar.last_value {
                let prev_formatted = stat_bar.data_type.format_value(prev_value);
                let prev_name = stat_bar.format.replace("{value}", &prev_formatted);
                if new_name == prev_name {
                    debug!(
                        "Skipping update for {} - formatted value unchanged",
                        stat_bar.channel_id
                    );
                    return Ok(());
                }
            }
        }

        debug!(
            "Updating channel {} to \"{}\"",
            stat_bar.channel_id, new_name
        );

        match timeout(
            Duration::from_secs(5),
            channel.edit(&ctx.http, EditChannel::default().name(&new_name)),
        )
        .await
        {
            Ok(Ok(_)) => {
                stat_bar.last_value = Some(value);
                stat_bar.last_update = Some(std::time::SystemTime::now());
                debug!(
                    "Updated stat bar {} to \"{}\"",
                    stat_bar.channel_id, new_name
                );
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Failed to update channel {}: {}", stat_bar.channel_id, e);
                Err(e.into())
            }
            Err(_) => {
                error!("Timeout updating channel {}", stat_bar.channel_id);
                Err("Channel update timeout".into())
            }
        }
    }
}

#[async_trait]
impl Task for StatsTask {
    fn name(&self) -> &str {
        "StatsUpdate"
    }

    fn schedule(&self) -> Option<Duration> {
        Some(Duration::from_secs(300))
    }

    async fn execute(
        &mut self,
        ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();
        info!("Starting stats update");

        let updates = self
            .db
            .read(|db| {
                let mut updates = Vec::new();
                for (guild_id, bars) in &db.stat_bars {
                    if let Some(settings) = db.guild_settings.get(guild_id) {
                        for stat_bar in bars.values() {
                            let should_update = if let Some(_last_value) = stat_bar.last_value {
                                let elapsed = stat_bar
                                    .last_update
                                    .and_then(|t| t.elapsed().ok())
                                    .map(|d| d.as_secs())
                                    .unwrap_or(u64::MAX);
                                elapsed >= settings.update_delay
                            } else {
                                true
                            };

                            if should_update {
                                updates.push((
                                    *guild_id,
                                    settings.prometheus_url.clone(),
                                    stat_bar.clone(),
                                ));
                            }
                        }
                    }
                }
                updates
            })
            .await;

        debug!("Processing {} stat bars", updates.len());

        let mut all_updates = Vec::new();

        for (guild_id, prometheus_url, mut stat_bar) in updates {
            sleep(Duration::from_millis(250)).await;

            match timeout(
                Duration::from_secs(10),
                Self::update_stat_bar(ctx, &prometheus_url, &mut stat_bar),
            )
            .await
            {
                Ok(Ok(_)) => all_updates.push((guild_id, stat_bar)),
                Ok(Err(e)) => error!("Failed to update stat bar {}: {}", stat_bar.channel_id, e),
                Err(_) => error!("Timeout updating stat bar {}", stat_bar.channel_id),
            }
        }

        if !all_updates.is_empty() {
            debug!("Writing updates for {} stat bars", all_updates.len());
            let write_start = std::time::Instant::now();

            self.db
                .transaction(|db| {
                    for (guild_id, stat_bar) in all_updates {
                        if let Some(bars) = db.stat_bars.get_mut(&guild_id) {
                            bars.insert(stat_bar.channel_id, stat_bar);
                        }
                    }
                    Ok(())
                })
                .await?;

            debug!("Database write completed in {:?}", write_start.elapsed());
        }

        info!("Stats update completed in {:?}", start.elapsed());
        Ok(())
    }

    fn box_clone(&self) -> Box<dyn Task> {
        Box::new(self.clone())
    }
}

impl Clone for StatsTask {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}
