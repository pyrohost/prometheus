use crate::database::Database;
use crate::modules::{lorax::database::LoraxDatabase, stats::database::StatsDatabase};

#[derive(Debug)]
pub struct Databases {
    pub lorax: Database<LoraxDatabase>,
    pub stats: Database<StatsDatabase>,
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
        })
    }
}
