use async_trait::async_trait;

use crate::core::werka::models::{
    DispatchRecord, WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
};

#[async_trait]
pub trait WerkaHomeLookup: Send + Sync {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError>;
    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError>;
    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError>;
    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError>;
    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError>;
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum WerkaPortError {
    #[error("lookup failed")]
    LookupFailed,
    #[error("database lookup failed: {0}")]
    Database(String),
}
