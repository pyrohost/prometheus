use crate::events::EventHandler;
use async_trait::async_trait;
use poise::serenity_prelude::{ActivityData, Context, FullEvent, OnlineStatus};

#[derive(Debug, Clone)]
pub struct ReadyHandler;

#[async_trait]
impl EventHandler for ReadyHandler {
    fn name(&self) -> &str {
        "Ready"
    }

    async fn handle(
        &self,
        ctx: &Context,
        event: &FullEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let FullEvent::Ready { .. } = event {
            ctx.set_presence(
                Some(ActivityData::watching("over pyro.host")),
                OnlineStatus::DoNotDisturb,
            )
        }
        Ok(())
    }

    fn box_clone(&self) -> Box<dyn EventHandler> {
        Box::new(self.clone())
    }
}
