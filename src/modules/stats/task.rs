use crate::tasks::Task;
use crate::{database::Database, modules::stats::database::StatsDatabase};
use async_trait::async_trait;
use poise::serenity_prelude::{ChannelId, Context, EditChannel};
use std::time::Duration;
use tracing::{debug, error, info, trace};

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
        trace!("Starting Prometheus query");
        debug!("Querying Prometheus - URL: {}, Query: {}", url, query);
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
        trace!("Sending HTTP request to Prometheus");
        let response = client
            .get(format!("{}/api/v1/query", url))
            .query(&[("query", query)])
            .send()
            .await?;

        debug!("Prometheus response time: {:?}", start.elapsed());
        trace!("Parsing Prometheus response");

        let response = response.json::<PrometheusResponse>().await?;

        if let Some(first_result) = response.data.result.first() {
            let value = first_result.value.1.parse::<f64>()?;
            debug!("Got value {} for query {}", value, query);
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
        debug!(
            "Updating stat bar for channel {} with query {}",
            stat_bar.channel_id, stat_bar.query
        );
        let start = std::time::Instant::now();

        let value = Self::query_prometheus(prometheus_url, &stat_bar.query).await?;
        debug!("Got value {} for query {}", value, stat_bar.query);

        let channel = ChannelId::new(stat_bar.channel_id);
        let formatted_value = stat_bar.data_type.format_value(value);
        let new_name = stat_bar.format.replace("{value}", &formatted_value);

        let channel_info = channel.to_channel(&ctx.http).await?;
        if let Some(current_name) = channel_info.guild().map(|c| c.name().to_string()) {
            if current_name == new_name {
                debug!("Channel name unchanged, skipping update");
                stat_bar.last_value = Some(value);
                return Ok(());
            }
        }

        debug!(
            "Updating channel {} name to: {}",
            stat_bar.channel_id, new_name
        );
        let edit_start = std::time::Instant::now();

        match channel
            .edit(&ctx.http, EditChannel::default().name(&new_name))
            .await
        {
            Ok(_) => {
                debug!("Channel edit took {:?}", edit_start.elapsed());
                stat_bar.last_value = Some(value);
                info!("Updated stat bar {} to {}", stat_bar.channel_id, new_name);
            }
            Err(e) => {
                error!("Failed to update channel {}: {}", stat_bar.channel_id, e);
                return Err(e.into());
            }
        }

        debug!("Stat bar update completed in {:?}", start.elapsed());
        Ok(())
    }
}

#[async_trait]
impl Task for StatsTask {
    fn name(&self) -> &str {
        "StatsUpdate"
    }

    fn schedule(&self) -> Option<Duration> {
        Some(Duration::from_secs(30))
    }

    async fn execute(
        &mut self,
        ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();
        info!("Starting stats update task");
        trace!("Beginning database read");

        let updates = self
            .db
            .read(|db| {
                debug!(
                    "Reading database, found {} guilds with stat bars",
                    db.stat_bars.len()
                );
                let mut updates = Vec::new();

                for (guild_id, bars) in &db.stat_bars {
                    if let Some(settings) = db.guild_settings.get(guild_id) {
                        let elapsed = start.elapsed().as_secs();
                        let should_update = bars.values().any(|bar| {
                            bar.last_value.is_none() || elapsed >= settings.update_delay
                        });

                        if should_update {
                            for stat_bar in bars.values() {
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

        debug!("Found {} guilds to update", updates.len());

        let mut all_updates = Vec::new();

        for (guild_id, prometheus_url, mut stat_bar) in updates {
            debug!("Processing guild {}", guild_id);
            let guild_start = std::time::Instant::now();

            if let Err(e) = Self::update_stat_bar(ctx, &prometheus_url, &mut stat_bar).await {
                error!("Failed to update stat bar {}: {}", stat_bar.channel_id, e);
                continue;
            }
            all_updates.push((guild_id, stat_bar));

            debug!(
                "Guild {} processed in {:?}",
                guild_id,
                guild_start.elapsed()
            );
        }

        if !all_updates.is_empty() {
            debug!("Writing updates for {} guilds", all_updates.len());
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

        info!("Stats update task completed in {:?}", start.elapsed());
        trace!("Task execution finished");
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
