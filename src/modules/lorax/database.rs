use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::{database::Database, default_struct};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoraxStage {
    Submission,
    Voting,
    Tiebreaker(usize),
    Completed,
    Inactive,
}

default_struct! {
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraxSettings {

    pub lorax_channel: Option<u64>,


    pub lorax_role: Option<u64>,
    pub winner_role: Option<u64>,
    pub alumni_role: Option<u64>,


    pub submission_duration: u64 = 60,
    pub voting_duration: u64 = 30,
    pub tiebreaker_duration: u64 = 15,
}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraxEvent {
    pub stage: LoraxStage,
    pub settings: LoraxSettings,
    pub tree_submissions: HashMap<u64, String>,
    pub tree_votes: HashMap<u64, String>,
    pub eliminated_trees: HashSet<String>,
    pub start_time: u64,
    pub current_trees: Vec<String>,
    pub campaign_message_id: Option<u64>,
    pub stage_message_id: Option<u64>,
    pub voting_message_id: Option<u64>,
    pub tiebreaker_message_id: Option<u64>,
    pub campaign_thread_id: Option<u64>,
}

impl LoraxEvent {
    pub fn new(settings: LoraxSettings, start_time: u64) -> Self {
        Self {
            stage: LoraxStage::Submission,
            settings,
            tree_submissions: HashMap::new(),
            tree_votes: HashMap::new(),
            eliminated_trees: HashSet::new(),
            start_time,
            current_trees: Vec::new(),
            campaign_message_id: None,
            stage_message_id: None,
            voting_message_id: None,
            tiebreaker_message_id: None,
            campaign_thread_id: None,
        }
    }

    pub fn get_stage_end_timestamp(&self, duration: u64) -> u64 {
        self.start_time + duration
    }

    pub fn get_tree_submitter(&self, tree_name: &str) -> Option<u64> {
        self.tree_submissions
            .iter()
            .find(|(_, name)| name.as_str() == tree_name)
            .map(|(uid, _)| *uid)
    }
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct LoraxDatabase {
    pub events: HashMap<u64, LoraxEvent>,
    pub settings: HashMap<u64, LoraxSettings>,
}

pub type LoraxHandler = Database<LoraxDatabase>;

impl LoraxHandler {
    pub async fn get_event(&self, guild_id: u64) -> Option<LoraxEvent> {
        self.read(|db| db.events.get(&guild_id).cloned()).await
    }

    pub async fn submit_tree(
        &self,
        guild_id: u64,
        tree: String,
        user_id: u64,
    ) -> Result<(bool, Option<String>), String> {
        self.write(|db| {
            if let Some(event) = db.events.get_mut(&guild_id) {
                let is_update = event.tree_submissions.contains_key(&user_id);
                let old_submission = event.tree_submissions.insert(user_id, tree);
                Ok((is_update, old_submission))
            } else {
                Err("No active event".to_string())
            }
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn vote_tree(
        &self,
        guild_id: u64,
        tree: String,
        user_id: u64,
    ) -> Result<bool, String> {
        self.write(|db| {
            if let Some(event) = db.events.get_mut(&guild_id) {
                let is_update = event.tree_votes.contains_key(&user_id);
                event.tree_votes.insert(user_id, tree);
                Ok(is_update)
            } else {
                Err("No active event".to_string())
            }
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn update_event(&self, guild_id: u64, event: LoraxEvent) -> Result<(), String> {
        self.write(|db| {
            db.events.insert(guild_id, event);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn get_settings(&self, guild_id: u64) -> Result<LoraxSettings, String> {
        Ok(self
            .read(|db| db.settings.get(&guild_id).cloned().unwrap_or_default())
            .await)
    }

    pub async fn ensure_settings(&self, guild_id: u64) -> Result<LoraxSettings, String> {
        self.write(|db| Ok(db.settings.entry(guild_id).or_default().clone()))
            .await
            .map_err(|e| e.to_string())
    }
}
