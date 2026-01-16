use async_trait::async_trait;
use time::Date;

use crate::core::werka::models::{
    CustomerDirectoryEntry, DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse,
    WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
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
    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError>;
    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError>;
    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError>;
    async fn werka_customers(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum WerkaPortError {
    #[error("lookup failed")]
    LookupFailed,
    #[error("database lookup failed: {0}")]
    Database(String),
}
