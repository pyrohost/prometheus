use crate::database::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ModrinthDatabase {
    pub linked_accounts: HashMap<u64, String>,
}

impl Database<ModrinthDatabase> {
    pub async fn link_account(&self, discord_id: u64, modrinth_id: String) -> Result<(), String> {
        self.transaction(|db| {
            db.linked_accounts.insert(discord_id, modrinth_id);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn unlink_account(&self, discord_id: u64) -> Result<(), String> {
        self.transaction(|db| {
            db.linked_accounts.remove(&discord_id);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn get_modrinth_id(&self, discord_id: u64) -> Option<String> {
        self.read(|db| db.linked_accounts.get(&discord_id).cloned())
            .await
    }
}
