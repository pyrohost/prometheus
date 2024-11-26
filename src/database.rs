use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{fs, sync::RwLock, time};
use tracing::error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Codec(String),
    #[error("Database error: {0}")]
    Custom(String),
}

/// A simple persistent database that stores serializable data
#[derive(Clone, Debug)]
pub struct Database<T: Serialize + DeserializeOwned + Default + Send + Sync + 'static> {
    data: Arc<RwLock<T>>,
    path: String,
}

impl<T: Serialize + DeserializeOwned + Default + Send + Sync + 'static> Database<T> {
    /// Creates a new database instance, loading existing data if available
    pub async fn new(path: impl Into<String>) -> Result<Self, DbError> {
        let path = path.into();

        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent).await?;
        }

        let db = Self {
            data: Arc::new(RwLock::new(T::default())),
            path,
        };

        if Path::new(&db.path).exists() {
            db.load().await?;
        }

        Ok(db)
    }

    /// Saves the current state to disk with retries
    async fn save(&self) -> Result<(), DbError> {
        const MAX_RETRIES: u32 = 3;
        const RETRY_DELAY: Duration = Duration::from_millis(100);

        for attempt in 1..=MAX_RETRIES {
            match self.try_save().await {
                Ok(()) => return Ok(()),
                Err(e) if attempt < MAX_RETRIES => {
                    error!("Save attempt {} failed: {}. Retrying...", attempt, e);
                    time::sleep(RETRY_DELAY).await;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    async fn try_save(&self) -> Result<(), DbError> {
        if let Some(parent) = Path::new(&self.path).parent() {
            fs::create_dir_all(parent).await?;
        }

        let data = self.data.read().await;
        let bytes = bincode::serialize(&*data).map_err(|e| DbError::Codec(e.to_string()))?;
        fs::write(&self.path, bytes).await?;
        Ok(())
    }

    /// Loads the state from disk
    async fn load(&self) -> Result<(), DbError> {
        let bytes = fs::read(&self.path).await?;
        let decoded = bincode::deserialize(&bytes).map_err(|e| DbError::Codec(e.to_string()))?;
        *self.data.write().await = decoded;
        Ok(())
    }

    /// Reads the database state with the provided function
    pub async fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let guard = self.data.read().await;
        f(&guard)
    }

    /// Modifies the database state and automatically saves changes
    pub async fn write<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut T) -> Result<R, String>,
    {
        let result;
        {
            let mut guard = self.data.write().await;
            result = f(&mut guard).map_err(DbError::Custom)?;
        }

        match time::timeout(Duration::from_secs(5), self.save()).await {
            Ok(save_result) => {
                save_result?;
                Ok(result)
            }
            Err(_) => {
                error!("Database save operation timed out");

                let data_clone = {
                    let guard = self.data.read().await;
                    bincode::serialize(&*guard).ok()
                };

                if let Some(bytes) = data_clone {
                    let path = self.path.clone();
                    tokio::spawn(async move {
                        if let Err(e) = fs::write(&path, bytes).await {
                            error!("Emergency save failed: {}", e);
                        }
                    });
                }

                Err(DbError::Custom(
                    "Database save operation timed out".to_string(),
                ))
            }
        }
    }
}
