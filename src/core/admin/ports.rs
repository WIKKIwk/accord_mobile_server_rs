use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::core::admin::models::{AdminDirectoryEntry, AdminState};
use crate::core::werka::models::SupplierItem;

#[async_trait]
pub trait AdminReadPort: Send + Sync {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError>;

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError>;

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError>;

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError>;

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError>;

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError>;

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError>;
}

#[async_trait]
pub trait AdminStatePort: Send + Sync {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AdminPortError {
    #[error("not found")]
    NotFound,
    #[error("lookup failed")]
    LookupFailed,
}
