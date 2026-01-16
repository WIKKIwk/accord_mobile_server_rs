use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use tokio::sync::Mutex;

use crate::core::auth::models::Principal;
use crate::core::session::models::SessionRecord;
use crate::error::AppError;
use crate::store::json_file;

#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<Mutex<SessionState>>,
}

struct SessionState {
    path: Option<PathBuf>,
    ttl_seconds: Option<u64>,
    loaded: bool,
    sessions: BTreeMap<String, SessionRecord>,
}

impl SessionManager {
    pub fn persistent(path: PathBuf, ttl_seconds: Option<u64>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SessionState {
                path: Some(path),
                ttl_seconds,
                loaded: false,
                sessions: BTreeMap::new(),
            })),
        }
    }

    #[allow(dead_code)]
    pub fn memory(ttl_seconds: Option<u64>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SessionState {
                path: None,
                ttl_seconds,
                loaded: true,
                sessions: BTreeMap::new(),
            })),
        }
    }

    #[allow(dead_code)]
    pub async fn create(&self, principal: Principal) -> Result<String, AppError> {
        let token = generate_token();
        let mut state = self.inner.lock().await;
        state.load().await?;

        let now = time::OffsetDateTime::now_utc();
        let record = SessionRecord::new(principal, now, state.ttl_seconds);
        state.sessions.insert(token.clone(), record);
        state.save().await?;

        Ok(token)
    }

    pub async fn get(&self, token: &str) -> Result<Principal, AppError> {
        let mut state = self.inner.lock().await;
        state.load().await?;

        let Some(record) = state.sessions.get(token).cloned() else {
            return Err(AppError::Unauthorized);
        };

        if record.is_expired(time::OffsetDateTime::now_utc()) {
            state.sessions.remove(token);
            state.save().await?;
            return Err(AppError::Unauthorized);
        }

        Ok(record.principal)
    }

    pub async fn delete(&self, token: &str) {
        let mut state = self.inner.lock().await;

        if state.load().await.is_ok() && state.sessions.remove(token).is_some() {
            let _ = state.save().await;
        }
    }
}

impl SessionState {
    async fn load(&mut self) -> Result<(), AppError> {
        if self.loaded {
            return Ok(());
        }

        self.sessions = match &self.path {
            Some(path) => json_file::read_map(path).await?,
            None => BTreeMap::new(),
        };
        self.drop_expired();
        self.loaded = true;

        Ok(())
    }

    async fn save(&self) -> Result<(), AppError> {
        if let Some(path) = &self.path {
            json_file::write_pretty(path, &self.sessions).await?;
        }

        Ok(())
    }

    fn drop_expired(&mut self) {
        let now = time::OffsetDateTime::now_utc();
        self.sessions.retain(|_, record| !record.is_expired(now));
    }
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::generate_token;

    #[test]
    fn token_matches_go_length() {
        assert_eq!(generate_token().len(), 32);
    }
}
