use futures::future::join_all;
use futures::StreamExt;
use poise::serenity_prelude::Context;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;

#[async_trait::async_trait]
pub trait Task: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn schedule(&self) -> Option<Duration>;
    async fn execute(
        &mut self,
        ctx: &Context,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn box_clone(&self) -> Box<dyn Task>;
}

impl Clone for Box<dyn Task> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[derive(Debug)]
pub struct TaskManager {
    tasks: Mutex<Vec<Box<dyn Task>>>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            tasks: Mutex::new(Vec::new()),
            handles: Mutex::new(Vec::new()),
            shutdown_tx,
        }
    }

    pub async fn add_task(&self, task: impl Task + 'static) {
        self.tasks.lock().await.push(Box::new(task));
    }

    pub async fn start_tasks(&self, ctx: Context) {
        let mut tasks = self.tasks.lock().await;
        let mut handles = self.handles.lock().await;

        for chunk in tasks.drain(..).collect::<Vec<_>>().chunks(10) {
            let tasks_chunk = chunk.iter().map(|t| t.box_clone()).collect::<Vec<_>>();
            let ctx = ctx.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            let handle = tokio::spawn(async move {
                let mut intervals = futures::stream::FuturesUnordered::new();

                for mut task in tasks_chunk {
                    if let Some(interval) = task.schedule() {
                        let ctx = ctx.clone();
                        intervals.push(tokio::spawn(async move {
                            loop {
                                task.execute(&ctx).await.ok();
                                tokio::time::sleep(interval).await;
                            }
                        }));
                    }
                }

                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        for interval in intervals {
                            interval.abort();
                        }
                    }
                    _ = async {
                        while intervals.next().await.is_some() {}
                    } => {}
                }
            });
            handles.push(handle);
        }
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        let mut handles = self.handles.lock().await;
        join_all(handles.iter_mut()).await;
    }
}
