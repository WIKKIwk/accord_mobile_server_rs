use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;

use crate::core::auth::models::Principal;
use crate::core::session::models::SessionRecord;
use crate::core::session::store::{JsonSessionStore, SessionStore};
use crate::error::AppError;

#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    ttl_seconds: Option<u64>,
}

impl SessionManager {
    pub fn persistent(path: PathBuf, ttl_seconds: Option<u64>) -> Self {
        Self::with_store(Arc::new(JsonSessionStore::persistent(path)), ttl_seconds)
    }

    #[allow(dead_code)]
    pub fn memory(ttl_seconds: Option<u64>) -> Self {
        Self::with_store(Arc::new(JsonSessionStore::memory()), ttl_seconds)
    }

    pub fn with_store(store: Arc<dyn SessionStore>, ttl_seconds: Option<u64>) -> Self {
        Self { store, ttl_seconds }
    }

    #[allow(dead_code)]
    pub async fn create(&self, principal: Principal) -> Result<String, AppError> {
        let token = generate_token();
        let now = time::OffsetDateTime::now_utc();
        self.store.delete_expired(now).await?;
        let record = SessionRecord::new(principal, now, None, self.ttl_seconds);
        self.store.put(&token, record).await?;
        Ok(token)
    }

    pub async fn get(&self, token: &str) -> Result<Principal, AppError> {
        let Some(record) = self.store.get(token).await? else {
            return Err(AppError::Unauthorized);
        };

        if record.is_expired(time::OffsetDateTime::now_utc()) {
            self.store.delete(token).await?;
            return Err(AppError::Unauthorized);
        }

        Ok(record.principal)
    }

    pub async fn delete(&self, token: &str) {
        let _ = self.store.delete(token).await;
    }

    pub async fn update(&self, token: &str, principal: Principal) {
        let Ok(Some(existing)) = self.store.get(token).await else {
            return;
        };

        let now = time::OffsetDateTime::now_utc();
        let record = SessionRecord::new(principal, now, existing.created_at, self.ttl_seconds);
        let _ = self.store.put(token, record).await;
    }
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::{SessionManager, generate_token};
    use crate::core::auth::models::{Principal, PrincipalRole};

    #[test]
    fn token_matches_go_length() {
        assert_eq!(generate_token().len(), 32);
    }

    #[tokio::test]
    async fn update_replaces_principal() {
        let sessions = SessionManager::memory(Some(60));
        let token = sessions
            .create(Principal {
                role: PrincipalRole::Admin,
                display_name: "Admin".to_string(),
                legal_name: "Admin".to_string(),
                ref_: "admin".to_string(),
                phone: "+998880000000".to_string(),
                avatar_url: String::new(),
            })
            .await
            .expect("create session");

        sessions
            .update(
                &token,
                Principal {
                    role: PrincipalRole::Admin,
                    display_name: "Alias".to_string(),
                    legal_name: "Admin".to_string(),
                    ref_: "admin".to_string(),
                    phone: "+998880000000".to_string(),
                    avatar_url: String::new(),
                },
            )
            .await;

        let principal = sessions.get(&token).await.expect("get session");
        assert_eq!(principal.display_name, "Alias");
    }
}
