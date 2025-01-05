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

    pub fn get_winner(&self) -> Option<String> {
        let mut vote_counts: std::collections::HashMap<&String, usize> = std::collections::HashMap::new();
        
        for voted_tree in self.tree_votes.values() {
            *vote_counts.entry(voted_tree).or_insert(0) += 1;
        }

        if vote_counts.is_empty() {
            return None;
        }

        vote_counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(tree, _)| tree.clone())
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
        self.get_data().await.events.get(&guild_id).cloned()
    }

    pub async fn submit_tree(
        &self,
        guild_id: u64,
        tree: String,
        user_id: u64,
    ) -> Result<(bool, Option<String>), String> {
        if tree.trim().is_empty() {
            return Err("Tree name cannot be empty".to_string());
        }

        if tree.len() > 32 {
            return Err("Tree name cannot be longer than 32 characters".to_string());
        }

        let tree = tree.trim().to_owned();

        self.transaction(|db| {
            let event = db.events.get_mut(&guild_id)
                .ok_or("No active event")?;
            
            if !matches!(event.stage, LoraxStage::Submission) {
                return Err("Submissions are not currently open".to_string());
            }

            // Check for duplicate names
            if event.tree_submissions.values().any(|t| t.eq_ignore_ascii_case(&tree)) {
                return Err("That tree name has already been submitted".to_string());
            }

            if event.eliminated_trees.contains(&tree.to_lowercase()) {
                return Err("That tree name has been disqualified".to_string());
            }

            let is_update = event.tree_submissions.contains_key(&user_id);
            let old_submission = event.tree_submissions.insert(user_id, tree);
            Ok((is_update, old_submission))
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
        self.transaction(|db| {
            let event = db.events.get_mut(&guild_id)
                .ok_or("No active event")?;

            if !matches!(event.stage, LoraxStage::Voting | LoraxStage::Tiebreaker(_)) {
                return Err("Voting is not currently open".to_string());
            }

            if !event.current_trees.iter().any(|t| t.eq_ignore_ascii_case(&tree)) {
                return Err("Invalid tree selection".to_string());
            }

            let is_update = event.tree_votes.contains_key(&user_id);
            event.tree_votes.insert(user_id, tree);
            Ok(is_update)
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn update_event(&self, guild_id: u64, event: LoraxEvent) -> Result<(), String> {
        self.transaction(|db| {
            db.events.insert(guild_id, event);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn get_settings(&self, guild_id: u64) -> Result<LoraxSettings, String> {
        Ok(self
            .get_data()
            .await
            .settings
            .get(&guild_id)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn ensure_settings(&self, guild_id: u64) -> Result<LoraxSettings, String> {
        self.transaction(|db| Ok(db.settings.entry(guild_id).or_default().clone()))
            .await
            .map_err(|e| e.to_string())
    }
}
