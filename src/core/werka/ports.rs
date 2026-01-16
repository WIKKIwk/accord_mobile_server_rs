use async_trait::async_trait;
use time::Date;

use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, SupplierDirectoryEntry,
    SupplierItem, WerkaArchiveResponse, WerkaCustomerIssueRecord, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};

#[async_trait]
pub trait WerkaHomeLookup: Send + Sync {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary::default())
    }
    async fn werka_home(&self, _pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        Ok(WerkaHomeData::default())
    }
    async fn werka_pending(&self, _limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_status_breakdown(
        &self,
        _kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_status_details(
        &self,
        _kind: &str,
        _supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<Date>,
        _to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }
    async fn werka_suppliers(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_customers(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_supplier_items(
        &self,
        _supplier_ref: &str,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
    async fn werka_customer_item_options(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErpItem {
    pub code: String,
    pub name: String,
    pub uom: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreateDeliveryNoteInput {
    pub customer: String,
    pub company: String,
    pub warehouse: String,
    pub item_code: String,
    pub qty: f64,
    pub uom: String,
    pub source_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryNoteStateUpdate {
    pub flow_state: String,
    pub customer_state: String,
    pub customer_reason: String,
    pub delivery_actor: String,
    pub ui_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeliveryNoteDraft {
    pub name: String,
    pub remarks: String,
    pub accord_source_key: String,
}

#[async_trait]
pub trait WerkaCustomerIssueWriter: Send + Sync {
    async fn get_items_by_codes(&self, codes: &[String]) -> Result<Vec<ErpItem>, WerkaPortError>;
    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError>;
    async fn resolve_company(&self) -> Result<String, WerkaPortError>;
    async fn customer_issue_source_exists_by_scan(
        &self,
        customer_ref: &str,
        marker: &str,
    ) -> Result<bool, WerkaPortError>;
    async fn create_draft_delivery_note(
        &self,
        input: CreateDeliveryNoteInput,
    ) -> Result<String, WerkaPortError>;
    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), WerkaPortError>;
    async fn submit_delivery_note(&self, name: &str) -> Result<(), WerkaPortError>;
    async fn delete_delivery_note(&self, name: &str) -> Result<(), WerkaPortError>;
}

#[async_trait]
pub trait CustomerIssueSourceLookup: Send + Sync {
    async fn customer_issue_source_exists(&self, marker: &str) -> Result<bool, WerkaPortError>;
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum WerkaPortError {
    #[error("lookup failed")]
    LookupFailed,
    #[error("database lookup failed: {0}")]
    Database(String),
    #[error("invalid input")]
    InvalidInput,
    #[error("insufficient stock")]
    InsufficientStock,
    #[error("duplicate customer issue source")]
    DuplicateCustomerIssueSource,
    #[error("write failed: {0}")]
    WriteFailed(String),
}

#[allow(dead_code)]
fn _customer_issue_record_contract(_record: WerkaCustomerIssueRecord) {}
