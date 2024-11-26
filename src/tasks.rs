use std::{sync::Arc, time::Duration};
use tokio::{sync::RwLock, time};
use tracing::error;
use poise::serenity_prelude::Context;

#[async_trait::async_trait]
pub trait Task: Send + Sync {
    fn name(&self) -> &str;
    fn schedule(&self) -> Option<Duration>;
    async fn execute(&self, ctx: &Context) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn box_clone(&self) -> Box<dyn Task>;
}

pub struct TaskManager {
    tasks: Arc<RwLock<Vec<Box<dyn Task>>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn add_task<T: Task + 'static>(&self, task: T) {
        let mut tasks = self.tasks.write().await;
        tasks.push(Box::new(task));
    }

    pub async fn start(&self, ctx: Arc<Context>) {
        let tasks = self.tasks.clone();

        tokio::spawn(async move {
            let tasks = tasks.read().await;
            let ctx = ctx.clone();

            for task in tasks.iter() {
                if let Some(duration) = task.schedule() {
                    let ctx = ctx.clone();
                    let task = task.box_clone();
                    tokio::spawn(async move {
                        let mut interval = time::interval(duration);
                        loop {
                            interval.tick().await;
                            if let Err(e) = task.execute(&ctx).await {
                                error!("Task '{}' failed: {}", task.name(), e);
                            }
                        }
                    });
                }
            }
        });
    }
}
