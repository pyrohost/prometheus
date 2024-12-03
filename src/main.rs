use crate::modules::lorax::database::LoraxHandler;
use databases::Databases;
use modules::{
    lorax::{commands::lorax, task::LoraxEventTask},
    stats::{stats, task::StatsTask},
    system::events::ReadyHandler,
};
use poise::serenity_prelude::{self as serenity, CreateAllowedMentions};
use std::sync::Arc;
use tasks::TaskManager;
use tracing::{error, info, trace};

mod database;
mod databases;
mod events;
mod modules;
mod tasks;
mod utils;

use crate::events::EventManager;

#[derive(Clone, Debug)]
pub struct Data {
    pub dbs: Arc<Databases>,
    pub task_manager: Arc<TaskManager>,
    pub event_manager: Arc<EventManager>,
}

impl Data {
    pub async fn init_tasks(&self, ctx: &serenity::Context) {
        let lorax_db = Arc::new(self.dbs.lorax.clone());
        let guild_ids: Vec<u64> = lorax_db
            .read(|db| db.events.keys().cloned().collect())
            .await;

        for guild_id in guild_ids {
            let lorax_task = LoraxEventTask::new(guild_id, lorax_db.clone());
            self.task_manager.add_task(lorax_task).await;
        }

        let stats_task = StatsTask::new(self.dbs.stats.clone());
        self.task_manager.add_task(stats_task).await;

        self.task_manager.start_tasks(ctx.clone()).await;
    }
}

impl LoraxHandler {
    pub async fn get_all_guild_ids(&self) -> Vec<u64> {
        self.read(|db| db.events.keys().cloned().collect()).await
    }
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
async fn register(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();
    info!("starting prometheus");

    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions::<Data, Error> {
            allowed_mentions: Some(CreateAllowedMentions::new().empty_roles().empty_users()),
            commands: vec![register(), lorax(), stats()],
            pre_command: |ctx| {
                Box::pin(async move {
                    trace!(
                        "Command {} used by {} in {}",
                        ctx.command().qualified_name,
                        ctx.author().tag(),
                        ctx.guild_id()
                            .map_or_else(|| "DM".to_string(), |id| id.to_string())
                    );
                })
            },
            post_command: |ctx| {
                Box::pin(async move {
                    info!(
                        "Command {} completed for {} in {}",
                        ctx.command().qualified_name,
                        ctx.author().tag(),
                        ctx.guild_id()
                            .map_or_else(|| "DM".to_string(), |id| id.to_string())
                    );
                })
            },
            on_error: |error| {
                Box::pin(async move {
                    match error {
                        poise::FrameworkError::Command { error, ctx, .. } => {
                            error!(
                                "Command {} failed for {} in {}: {:?}",
                                ctx.command().qualified_name,
                                ctx.author().tag(),
                                ctx.guild_id()
                                    .map_or_else(|| "DM".to_string(), |id| id.to_string()),
                                error
                            );
                        }
                        err => error!("Other framework error: {:?}", err),
                    }
                })
            },
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    data.event_manager.handle_event(ctx, &event).await;
                    Ok(())
                })
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                info!("registering commands");
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                let dbs = Arc::new(Databases::default().await?);
                let task_manager = Arc::new(tasks::TaskManager::new());
                let event_manager = Arc::new(events::EventManager::new());

                event_manager.add_handler(ReadyHandler).await;

                let data = Data {
                    dbs,
                    task_manager,
                    event_manager,
                };
                data.init_tasks(ctx).await;

                Ok(data)
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;

    client.unwrap().start().await.unwrap();
}
