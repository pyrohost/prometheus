use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RecordingDatabase {
    pub channels: HashMap<u64, RecordingChannel>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RecordingChannel {
    pub guild_id: u64,
    pub voice_channel_id: u64,
    pub is_recording: bool,
    pub last_activity: Option<chrono::DateTime<chrono::Utc>>,
}
