use crate::database::Database;

#[derive(Debug)]
pub struct Databases {
    pub lorax: Database<crate::modules::lorax::database::LoraxDatabase>,
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
