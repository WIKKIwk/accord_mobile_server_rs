use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::{SerdeJson, Str};
use heed::{Database, Env, EnvOpenOptions};
use tokio::sync::Mutex;

use crate::core::session::models::SessionRecord;
use crate::error::AppError;
use crate::store::json_file;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError>;
    async fn put(&self, token: &str, record: SessionRecord) -> Result<(), AppError>;
    async fn delete(&self, token: &str) -> Result<(), AppError>;
}

#[derive(Clone)]
pub struct JsonSessionStore {
    path: Option<PathBuf>,
    state: Arc<Mutex<JsonSessionState>>,
}

#[derive(Default)]
struct JsonSessionState {
    loaded: bool,
    sessions: BTreeMap<String, SessionRecord>,
}

impl JsonSessionStore {
    pub fn persistent(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            state: Arc::new(Mutex::new(JsonSessionState::default())),
        }
    }

    pub fn memory() -> Self {
        Self {
            path: None,
            state: Arc::new(Mutex::new(JsonSessionState {
                loaded: true,
                sessions: BTreeMap::new(),
            })),
        }
    }

    async fn load_if_needed(&self, state: &mut JsonSessionState) -> Result<(), AppError> {
        if state.loaded {
            return Ok(());
        }
        state.sessions = match &self.path {
            Some(path) => json_file::read_map(path).await?,
            None => BTreeMap::new(),
        };
        let now = time::OffsetDateTime::now_utc();
        state.sessions.retain(|_, record| !record.is_expired(now));
        state.loaded = true;
        Ok(())
    }

    async fn save(&self, state: &JsonSessionState) -> Result<(), AppError> {
        if let Some(path) = &self.path {
            json_file::write_pretty(path, &state.sessions).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl SessionStore for JsonSessionStore {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        Ok(state.sessions.get(token).cloned())
    }

    async fn put(&self, token: &str, record: SessionRecord) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        state.sessions.insert(token.to_string(), record);
        self.save(&state).await
    }

    async fn delete(&self, token: &str) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        if state.sessions.remove(token).is_some() {
            self.save(&state).await?;
        }
        Ok(())
    }
}

pub struct LmdbSessionStore {
    env: Env,
    db: Database<Str, SerdeJson<SessionRecord>>,
    write_lock: Arc<Mutex<()>>,
}

impl LmdbSessionStore {
    pub fn open(path: PathBuf, map_size_bytes: usize) -> Result<Self, AppError> {
        std::fs::create_dir_all(&path)?;
        let map_size = map_size_bytes.max(1024 * 1024);
        let env = unsafe {
            // LMDB requires the caller to ensure the environment path is used
            // consistently. This service owns this directory for sessions only.
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(2)
                .open(&path)
        }
        .map_err(lmdb_error)?;
        let mut wtxn = env.write_txn().map_err(lmdb_error)?;
        let db = env
            .create_database(&mut wtxn, Some("sessions"))
            .map_err(lmdb_error)?;
        wtxn.commit().map_err(lmdb_error)?;

        Ok(Self {
            env,
            db,
            write_lock: Arc::new(Mutex::new(())),
        })
    }
}

#[async_trait]
impl SessionStore for LmdbSessionStore {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError> {
        let rtxn = self.env.read_txn().map_err(lmdb_error)?;
        self.db.get(&rtxn, token).map_err(lmdb_error)
    }

    async fn put(&self, token: &str, record: SessionRecord) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_error)?;
        self.db.put(&mut wtxn, token, &record).map_err(lmdb_error)?;
        wtxn.commit().map_err(lmdb_error)
    }

    async fn delete(&self, token: &str) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_error)?;
        self.db.delete(&mut wtxn, token).map_err(lmdb_error)?;
        wtxn.commit().map_err(lmdb_error)
    }
}

fn lmdb_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb session store failed: {error}"))
}
