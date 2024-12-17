use crate::{database::Database, default_struct};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt};

#[derive(Debug, Clone, Serialize, Deserialize, poise::ChoiceParameter)]
pub enum DataType {
    #[name = "Integer (123)"]
    Integer,
    #[name = "Float (123.45)"]
    Float,
    #[name = "Percentage (12.34%)"]
    Percentage,
    #[name = "Bytes (1.23 GB)"]
    Bytes,
    #[name = "Duration (1d 2h)"]
    Duration,
    #[name = "Temperature (23.4°C)"]
    Temperature,
    #[name = "Speed (123 MB/s)"]
    Speed,
    #[name = "Currency ($123.45)"]
    Currency,
    #[name = "Scientific (1.23e4)"]
    Scientific,
}

impl DataType {
    pub fn format_value(&self, value: f64) -> String {
        match self {
            Self::Integer => format!("{}", value as i64),
            Self::Float => format!("{:.2}", value),
            Self::Percentage => format!("{:.1}%", value),
            Self::Bytes => {
                const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
                let mut value = value;
                let mut unit_idx = 0;

                while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
                    value /= 1024.0;
                    unit_idx += 1;
                }

                format!("{:.1} {}", value, UNITS[unit_idx])
            }
            Self::Duration => {
                let secs = value as i64;
                let days = secs / 86400;
                let hours = (secs % 86400) / 3600;
                let mins = (secs % 3600) / 60;

                if days > 0 {
                    format!("{}d {}h", days, hours)
                } else if hours > 0 {
                    format!("{}h {}m", hours, mins)
                } else {
                    format!("{}m", mins)
                }
            }
            Self::Temperature => format!("{:.1}°C", value),
            Self::Speed => {
                let (scaled, unit) = if value >= 1_000_000_000.0 {
                    (value / 1_000_000_000.0, "GB/s")
                } else if value >= 1_000_000.0 {
                    (value / 1_000_000.0, "MB/s")
                } else if value >= 1_000.0 {
                    (value / 1_000.0, "KB/s")
                } else {
                    (value, "B/s")
                };
                format!("{:.1} {}", scaled, unit)
            }
            Self::Currency => format!("${:.2}", value),
            Self::Scientific => format!("{:e}", value),
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer => write!(f, "integer"),
            Self::Float => write!(f, "float"),
            Self::Percentage => write!(f, "percentage"),
            Self::Bytes => write!(f, "bytes"),
            Self::Duration => write!(f, "duration"),
            Self::Temperature => write!(f, "temperature"),
            Self::Speed => write!(f, "speed"),
            Self::Currency => write!(f, "currency"),
            Self::Scientific => write!(f, "scientific"),
        }
    }
}

default_struct! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildSettings {
    pub prometheus_url: String = String::new(),
    pub update_delay: u64 = 60,
}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatBar {
    pub channel_id: u64,
    pub query: String,
    pub format: String,
    pub data_type: DataType,
    pub last_value: Option<f64>,
    pub last_update: Option<std::time::SystemTime>,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct StatsDatabase {
    pub stat_bars: HashMap<u64, HashMap<u64, StatBar>>,
    pub guild_settings: HashMap<u64, GuildSettings>,
}

impl Database<StatsDatabase> {
    pub async fn get_settings(&self, guild_id: u64) -> Result<GuildSettings, String> {
        Ok(self
            .read(|db| {
                db.guild_settings
                    .get(&guild_id)
                    .cloned()
                    .unwrap_or_default()
            })
            .await)
    }

    pub async fn ensure_settings(&self, guild_id: u64) -> Result<GuildSettings, String> {
        self.transaction(|db| Ok(db.guild_settings.entry(guild_id).or_default().clone()))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_stat_bars(&self, guild_id: u64) -> Result<Vec<StatBar>, String> {
        Ok(self
            .read(|db| {
                db.stat_bars
                    .get(&guild_id)
                    .map(|bars| bars.values().cloned().collect())
                    .unwrap_or_default()
            })
            .await)
    }

    pub async fn update_stat_bar(&self, guild_id: u64, bar: StatBar) -> Result<(), String> {
        self.transaction(|db| {
            db.stat_bars
                .entry(guild_id)
                .or_default()
                .insert(bar.channel_id, bar);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }
}
