use crate::database::Database;
use crate::tasks::Task;
use async_trait::async_trait;
use poise::serenity_prelude::Context;
use std::time::{Duration, SystemTime};
use tracing::{error, info};

use super::database::TestingDatabase;

#[derive(Debug)]
pub struct TestingTask {
    db: Database<TestingDatabase>,
    master_key: String,
}

impl TestingTask {
    pub fn new(db: Database<TestingDatabase>, master_key: String) -> Self {
        Self { db, master_key }
    }

    async fn delete_server(
        &self,
        server_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::new();
        client
            .delete(format!(
                "https://archon.pyro.host/modrinth/v0/servers/{}/delete",
                server_id
            ))
            .header("X-MASTER-KEY", &self.master_key)
            .send()
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Task for TestingTask {
    fn name(&self) -> &str {
        "TestingCleanup"
    }

    fn schedule(&self) -> Option<Duration> {
        Some(Duration::from_secs(300))
    }

    async fn execute(
        &mut self,
        _ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting testing servers cleanup");
        let now = SystemTime::now();

        let expired = self
            .db
            .read(|db| {
                db.servers
                    .values()
                    .filter(|s| s.expires_at <= now)
                    .map(|s| s.server_id.clone())
                    .collect::<Vec<_>>()
            })
            .await;

        for server_id in expired {
            match self.delete_server(&server_id).await {
                Ok(_) => {
                    if let Err(e) = self.db.remove_server(&server_id).await {
                        error!("Failed to remove server from database: {}", e);
                    }
                }
                Err(e) => error!("Failed to delete server {}: {}", server_id, e),
            }
        }

        Ok(())
    }

    fn box_clone(&self) -> Box<dyn Task> {
        Box::new(Self {
            db: self.db.clone(),
            master_key: self.master_key.clone(),
        })
    }
}
