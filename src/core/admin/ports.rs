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
    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError>;
}

#[async_trait]
pub trait AdminCredentialPort: Send + Sync {
    async fn admin_api_auth(&self, username: &str) -> Result<(String, String), AdminPortError>;

    async fn update_admin_api_auth(
        &self,
        username: &str,
        api_key: &str,
        api_secret: &str,
    ) -> Result<(), AdminPortError>;
}

pub trait AdminEnvPersister: Send + Sync {
    fn upsert(&self, values: BTreeMap<&'static str, String>) -> Result<(), AdminPortError>;
}

pub trait AdminErpConfigSink: Send + Sync {
    fn set_erp_config(
        &self,
        base_url: &str,
        api_key: &str,
        api_secret: &str,
        default_warehouse: &str,
    );
}

#[async_trait]
pub trait AdminWritePort: Send + Sync {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError>;

    async fn assign_supplier_item(&self, ref_: &str, item_code: &str)
    -> Result<(), AdminPortError>;

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError>;

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError>;

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError>;

    async fn assign_customer_item(&self, ref_: &str, item_code: &str)
    -> Result<(), AdminPortError>;

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError>;

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError>;

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AdminPortError {
    #[error("not found")]
    NotFound,
    #[error("lookup failed")]
    LookupFailed,
    #[error("code regenerate cooldown")]
    CodeRegenCooldown,
    #[error("{0}")]
    InvalidInput(String),
}
