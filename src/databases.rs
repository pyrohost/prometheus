use crate::database::Database;
use crate::modules::{
    lorax::database::LoraxDatabase, modrinth::database::ModrinthDatabase,
    stats::database::StatsDatabase, testing::database::TestingDatabase,
};

#[derive(Debug)]
pub struct Databases {
    pub lorax: Database<LoraxDatabase>,
    pub stats: Database<StatsDatabase>,
    pub testing: Database<TestingDatabase>,
    pub modrinth: Database<ModrinthDatabase>,
}

impl Default for Databases {
    fn default() -> Self {
        unimplemented!("Use Databases::default() async function instead")
    }
}

impl Databases {
    pub async fn default() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            lorax: Database::new("data/lorax.db").await?,
            stats: Database::new("data/stats.db").await?,
            testing: Database::new("data/testing.db").await?,
            modrinth: Database::new("modrinth.json").await?,
        })
    }
}
