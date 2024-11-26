use crate::database::Database;
use crate::modules::lorax::database::LoraxDatabase;

pub struct Databases {
    pub lorax: Database<LoraxDatabase>,
}

impl Databases {
    pub async fn default() -> Result<Self, crate::database::DbError> {
        Ok(Self {
            lorax: Database::new("data/lorax.db").await?,
        })
    }
}
