use crate::modules::lorax::database::LoraxHandler;
use databases::Databases;
use modules::lorax::task::LoraxEventTask;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use tracing::info;

mod database;
mod databases;
mod modules;
mod tasks;
mod utils;

#[derive(Clone)]
pub struct Data {
    pub dbs: Arc<Databases>,
    pub task_manager: Arc<tasks::TaskManager>,
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
            commands: vec![register(), modules::lorax::commands::lorax()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                info!("registering commands");
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                let dbs = Arc::new(Databases::default().await?);
                let task_manager = Arc::new(tasks::TaskManager::new());
                let data = Data { dbs, task_manager };
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
