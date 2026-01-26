use async_trait::async_trait;

use crate::core::push::models::PushTokenRecord;

#[async_trait]
pub trait PushTokenStorePort: Send + Sync {
    async fn move_token_to_key(
        &self,
        target_key: &str,
        token: &str,
        platform: &str,
    ) -> Result<(), PushStoreError>;

    async fn delete(&self, key: &str, token: &str) -> Result<(), PushStoreError>;

    async fn list(&self, key: &str) -> Result<Vec<PushTokenRecord>, PushStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PushStoreError {
    #[error("store failed")]
    StoreFailed,
}

#[derive(Debug, thiserror::Error)]
pub enum PushServiceError {
    #[error("token is required")]
    TokenRequired,
    #[error("store failed")]
    StoreFailed,
}

impl From<PushStoreError> for PushServiceError {
    fn from(_: PushStoreError) -> Self {
        Self::StoreFailed
    }
}
