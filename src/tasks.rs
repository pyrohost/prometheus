use futures::future::join_all;
use poise::serenity_prelude::Context;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[async_trait::async_trait]
pub trait Task: Send + Sync {
    fn name(&self) -> &str;
    fn schedule(&self) -> Option<Duration>;
    async fn execute(
        &mut self,
        ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn box_clone(&self) -> Box<dyn Task>;
}

pub struct TaskManager {
    tasks: Mutex<Vec<Box<dyn Task>>>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            handles: Mutex::new(Vec::new()),
        }
    }

    pub async fn add_task(&self, task: impl Task + 'static) {
        self.tasks.lock().await.push(Box::new(task));
    }

    pub async fn start_tasks(&self, ctx: Context) {
        let mut tasks = self.tasks.lock().await;
        let mut handles = self.handles.lock().await;

        for task in tasks.drain(..) {
            let ctx = ctx.clone();
            let handle = tokio::spawn(async move {
                let mut task = task;
                while let Some(interval) = task.schedule() {
                    task.execute(&ctx).await.ok();
                    tokio::time::sleep(interval).await;
                }
            });
            handles.push(handle);
        }
    }

    pub async fn shutdown(&self) {
        let mut handles = self.handles.lock().await;
        for handle in handles.iter_mut() {
            handle.abort();
        }
        join_all(handles.iter_mut()).await;
    }
}
