use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Arc};
use thiserror::Error;
use tokio::{fs, sync::RwLock};
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
#[derive(Clone)]
pub struct Database<T: Serialize + DeserializeOwned + Default> {
    data: Arc<RwLock<T>>,
    path: String,
}

impl<T: Serialize + DeserializeOwned + Default> Database<T> {
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

    /// Saves the current state to disk
    pub async fn save(&self) -> Result<(), DbError> {
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
        let mut guard = self.data.write().await;
        let result = f(&mut guard).map_err(DbError::Custom)?;
        self.save().await?;
        Ok(result)
    }
}
