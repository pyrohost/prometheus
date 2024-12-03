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

#[derive(Debug)]
struct DatabaseInner<T> {
    data: T,
    path: String,
}

#[derive(Clone, Debug)]
pub struct Database<T: Serialize + DeserializeOwned + Default + Send + Sync + Clone + 'static> {
    inner: Arc<RwLock<DatabaseInner<T>>>,
}

impl<T: Serialize + DeserializeOwned + Default + Send + Sync + Clone + 'static> Database<T> {
    pub async fn new(path: impl Into<String>) -> Result<Self, DbError> {
        let path = path.into();

        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                error!("Failed to create database directory: {}", e);
                DbError::Io(e)
            })?;
        }

        let data = if Path::new(&path).exists() {
            match fs::read(&path).await {
                Ok(bytes) => match bincode::deserialize(&bytes) {
                    Ok(data) => data,
                    Err(e) => {
                        error!("Failed to deserialize database {}: {}", path, e);
                        T::default()
                    }
                },
                Err(e) => {
                    error!("Failed to read database {}: {}", path, e);
                    T::default()
                }
            }
        } else {
            T::default()
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(DatabaseInner { data, path })),
        })
    }

    async fn save(&self, data: &T) -> Result<(), DbError> {
        let path = {
            let guard = self.inner.read().await;
            guard.path.clone()
        };

        let bytes = bincode::serialize(data).map_err(|e| DbError::Codec(e.to_string()))?;

        match time::timeout(Duration::from_secs(5), fs::write(&path, bytes)).await {
            Ok(result) => Ok(result?),
            Err(_) => {
                error!("Database save operation timed out");
                Err(DbError::Custom("Save operation timed out".into()))
            }
        }
    }

    pub async fn get_data(&self) -> T {
        let guard = self.inner.read().await;
        guard.data.clone()
    }

    pub async fn transaction<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut T) -> Result<R, String>,
    {
        let mut data = self.get_data().await;
        let result = f(&mut data).map_err(DbError::Custom)?;

        self.save(&data).await?;

        let mut guard = self.inner.write().await;
        guard.data = data;

        Ok(result)
    }

    pub async fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let guard = self.inner.read().await;
        f(&guard.data)
    }
}
