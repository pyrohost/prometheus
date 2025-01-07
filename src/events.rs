use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use poise::serenity_prelude::{Context, FullEvent};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::{Data, modules::recording::handler::RecordingHandler};

#[async_trait]
pub trait EventHandler: Send + Sync + Debug {
    fn name(&self) -> &str;
    async fn handle(
        &self,
        ctx: &Context,
        event: &FullEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn box_clone(&self) -> Box<dyn EventHandler>;
}

impl Clone for Box<dyn EventHandler> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[derive(Debug, Default)]
pub struct EventManager {
    handlers: Mutex<Vec<Box<dyn EventHandler>>>,
}

impl EventManager {
    pub fn new() -> Self {
        Self {
            handlers: Mutex::new(Vec::new()),
        }
    }

    pub async fn init(&self, data: &Arc<Data>) {
        let mut handlers = self.handlers.lock().await;
        handlers.push(Box::new(RecordingHandler::new(data.dbs.recording.clone())));
    }

    pub async fn add_handler(&self, handler: impl EventHandler + 'static) {
        self.handlers.lock().await.push(Box::new(handler));
    }

    pub async fn handle_event(&self, ctx: &Context, event: &FullEvent) {
        let handlers = self.handlers.lock().await;
        let mut futures = FuturesUnordered::new();

        for handler in handlers.iter() {
            let handler = handler.box_clone();
            let ctx = ctx.clone();
            let event = event.clone();

            futures.push(tokio::spawn(async move {
                if let Err(e) = handler.handle(&ctx, &event).await {
                    tracing::error!("Error in event handler {}: {}", handler.name(), e);
                }
            }));
        }

        while futures.next().await.is_some() {}
    }
}
