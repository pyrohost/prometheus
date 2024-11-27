use crate::database::Database;
use crate::modules::lorax::database::LoraxDatabase;

#[derive(Debug)]
pub struct Databases {
    pub lorax: Database<LoraxDatabase>,
}

impl Default for Databases {
    fn default() -> Self {
        unimplemented!("Use Databases::default() async function instead")
    }
}

impl Databases {
    pub async fn default() -> Result<Self, crate::database::DbError> {
        Ok(Self {
            lorax: Database::new("data/lorax.db").await?,
        })
    }
}
