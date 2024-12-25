use crate::database::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestServer {
    pub server_id: String,
    pub user_id: u64,
    pub name: String,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TestingDatabase {
    pub servers: HashMap<String, TestServer>,
    pub user_limits: HashMap<u64, usize>,
}

impl Database<TestingDatabase> {
    pub async fn get_user_server(&self, user_id: u64) -> Option<TestServer> {
        self.read(|db| db.servers.values().find(|s| s.user_id == user_id).cloned())
            .await
    }

    pub async fn add_server(&self, server: TestServer) -> Result<(), String> {
        self.transaction(|db| {
            db.servers.insert(server.server_id.clone(), server);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn remove_server(&self, server_id: &str) -> Result<(), String> {
        self.transaction(|db| {
            db.servers.remove(server_id);
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn extend_server(&self, server_id: &str, duration: Duration) -> Result<(), String> {
        self.transaction(|db| {
            if let Some(server) = db.servers.get_mut(server_id) {
                server.expires_at = SystemTime::now() + duration;
                Ok(())
            } else {
                Err("Server not found".to_string())
            }
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn get_user_servers(&self, user_id: u64) -> Vec<TestServer> {
        self.read(|db| {
            db.servers
                .values()
                .filter(|s| s.user_id == user_id)
                .cloned()
                .collect()
        })
        .await
    }

    pub async fn get_user_limit(&self, user_id: u64) -> usize {
        self.read(|db| db.user_limits.get(&user_id).cloned().unwrap_or(1))
            .await
    }

    pub async fn set_user_limit(&self, user_id: u64, limit: usize) -> Result<(), String> {
        self.transaction(|db| {
            if limit == 1 {
                db.user_limits.remove(&user_id);
            } else {
                db.user_limits.insert(user_id, limit);
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())
    }
}
