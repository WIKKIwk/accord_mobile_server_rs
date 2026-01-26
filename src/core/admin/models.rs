use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::core::werka::models::{CustomerDirectoryEntry, DispatchRecord, SupplierItem};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSettings {
    pub erp_url: String,
    pub erp_api_key: String,
    pub erp_api_secret: String,
    pub default_target_warehouse: String,
    pub default_uom: String,
    pub werka_phone: String,
    pub werka_name: String,
    pub werka_code: String,
    pub werka_code_locked: bool,
    pub werka_code_retry_after_sec: i64,
    pub admin_phone: String,
    pub admin_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplier {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
    pub code: String,
    pub blocked: bool,
    pub removed: bool,
    pub assigned_item_codes: Vec<String>,
    pub assigned_item_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSupplierSummary {
    pub total_suppliers: usize,
    pub active_suppliers: usize,
    pub blocked_suppliers: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminSuppliersPage {
    pub summary: AdminSupplierSummary,
    pub suppliers: Vec<AdminSupplier>,
    pub customers: Vec<CustomerDirectoryEntry>,
    pub settings: AdminSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminSupplierDetail {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
    pub code: String,
    pub blocked: bool,
    pub removed: bool,
    pub code_locked: bool,
    pub code_retry_after_sec: i64,
    pub assigned_items: Vec<SupplierItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AdminCustomerDetail {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub name: String,
    pub phone: String,
    pub code: String,
    pub code_locked: bool,
    pub code_retry_after_sec: i64,
    pub assigned_items: Vec<SupplierItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminDirectoryEntry {
    pub ref_: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminState {
    pub custom_code: String,
    pub blocked: bool,
    pub removed: bool,
    pub assigned_item_codes: Vec<String>,
    pub cooldown_until: Option<OffsetDateTime>,
}

impl AdminState {
    pub fn code_locked(&self, now: OffsetDateTime) -> bool {
        self.cooldown_until.is_some_and(|until| now < until)
    }

    pub fn retry_after_seconds(&self, now: OffsetDateTime) -> i64 {
        let Some(until) = self.cooldown_until else {
            return 0;
        };
        if now >= until {
            return 0;
        }
        let seconds = (until - now).whole_seconds();
        seconds.max(1)
    }
}

pub type AdminActivity = Vec<DispatchRecord>;
