use std::collections::BTreeMap;
use std::sync::Arc;

use time::OffsetDateTime;

use crate::config::AppConfig;
use crate::core::admin::models::{
    AdminActivity, AdminCustomerDetail, AdminDirectoryEntry, AdminSettings, AdminState,
    AdminSupplier, AdminSupplierDetail, AdminSupplierSummary, AdminSuppliersPage,
};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminStatePort};
use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierItem};

#[derive(Clone)]
pub struct AdminService {
    config: AdminConfig,
    read_port: Option<Arc<dyn AdminReadPort>>,
    state_port: Option<Arc<dyn AdminStatePort>>,
}

#[derive(Debug, Clone)]
struct AdminConfig {
    erp_url: String,
    erp_api_key: String,
    erp_api_secret: String,
    default_target_warehouse: String,
    werka_phone: String,
    werka_name: String,
    werka_code: String,
    admin_phone: String,
    admin_name: String,
}

impl AdminService {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: AdminConfig {
                erp_url: config.erp_url.clone(),
                erp_api_key: config.erp_api_key.clone(),
                erp_api_secret: config.erp_api_secret.clone(),
                default_target_warehouse: config.default_target_warehouse.clone(),
                werka_phone: "+99888862440".to_string(),
                werka_name: config.werka_name.clone(),
                werka_code: config.werka_code.clone(),
                admin_phone: config.admin_phone.clone(),
                admin_name: config.admin_name.clone(),
            },
            read_port: None,
            state_port: None,
        }
    }

    pub fn with_read_port(mut self, read_port: Arc<dyn AdminReadPort>) -> Self {
        self.read_port = Some(read_port);
        self
    }

    pub fn with_state_port(mut self, state_port: Arc<dyn AdminStatePort>) -> Self {
        self.state_port = Some(state_port);
        self
    }

    pub async fn settings(&self) -> Result<AdminSettings, AdminPortError> {
        let state = self.state_for("werka").await?;
        let now = OffsetDateTime::now_utc();
        Ok(AdminSettings {
            erp_url: self.config.erp_url.clone(),
            erp_api_key: self.config.erp_api_key.clone(),
            erp_api_secret: self.config.erp_api_secret.clone(),
            default_target_warehouse: self.config.default_target_warehouse.clone(),
            default_uom: "Kg".to_string(),
            werka_phone: self.config.werka_phone.clone(),
            werka_name: self.config.werka_name.clone(),
            werka_code: self.config.werka_code.clone(),
            werka_code_locked: state.code_locked(now),
            werka_code_retry_after_sec: state.retry_after_seconds(now),
            admin_phone: self.config.admin_phone.clone(),
            admin_name: self.config.admin_name.clone(),
        })
    }

    pub async fn suppliers_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.suppliers_page("", limit, offset).await?;
        self.admin_suppliers_from_entries(entries, &states)
    }

    pub async fn suppliers(&self, limit: usize) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.suppliers_page("", limit, 0).await?;
        self.admin_suppliers_from_entries(entries, &states)
    }

    pub async fn supplier_summary(
        &self,
        limit: usize,
    ) -> Result<AdminSupplierSummary, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.suppliers_page("", limit, 0).await?;
        let mut summary = AdminSupplierSummary {
            total_suppliers: entries.len(),
            ..AdminSupplierSummary::default()
        };
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if state.blocked || state.removed {
                summary.blocked_suppliers += 1;
            } else {
                summary.active_suppliers += 1;
            }
        }
        Ok(summary)
    }

    pub async fn suppliers_home(&self) -> Result<AdminSuppliersPage, AdminPortError> {
        let summary = self.supplier_summary(300).await?;
        let suppliers = self.suppliers(100).await?;
        let customers = self.customers(500).await.unwrap_or_default();
        let settings = self.settings().await?;
        Ok(AdminSuppliersPage {
            summary,
            suppliers,
            customers,
            settings,
        })
    }

    pub async fn inactive_suppliers(
        &self,
        limit: usize,
    ) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.suppliers_page("", limit, 0).await?;
        let mut result = Vec::new();
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if !state.blocked && !state.removed {
                continue;
            }
            result.push(self.build_supplier(entry, state)?);
        }
        Ok(result)
    }

    pub async fn supplier_detail(&self, ref_: &str) -> Result<AdminSupplierDetail, AdminPortError> {
        let read = self.read_port()?;
        let entry = read.supplier_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let assigned_items = match read.assigned_supplier_items(&entry.ref_, 200).await {
            Ok(items) => items,
            Err(_) => read
                .items_by_codes(&state.assigned_item_codes)
                .await
                .unwrap_or_default(),
        };
        let code = self.supplier_code(&entry, &state)?;
        let now = OffsetDateTime::now_utc();
        Ok(AdminSupplierDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            code,
            blocked: state.blocked,
            removed: state.removed,
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
            assigned_items,
        })
    }

    pub async fn assigned_supplier_items(
        &self,
        ref_: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.read_port()?
            .assigned_supplier_items(ref_.trim(), limit)
            .await
    }

    pub async fn customers(
        &self,
        limit: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.customers_page("", limit, 0).await?;
        Ok(entries
            .into_iter()
            .filter(|entry| {
                !states
                    .get(entry.ref_.trim())
                    .map(|state| state.removed)
                    .unwrap_or(false)
            })
            .map(customer_directory_entry)
            .collect())
    }

    pub async fn customers_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.customers_page("", limit, offset).await?;
        Ok(entries
            .into_iter()
            .filter(|entry| {
                !states
                    .get(entry.ref_.trim())
                    .map(|state| state.removed)
                    .unwrap_or(false)
            })
            .map(customer_directory_entry)
            .collect())
    }

    pub async fn customer_detail(&self, ref_: &str) -> Result<AdminCustomerDetail, AdminPortError> {
        let read = self.read_port()?;
        let entry = read.customer_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let assigned_items = read
            .customer_items(&entry.ref_, "", 200)
            .await
            .unwrap_or_default();
        let now = OffsetDateTime::now_utc();
        Ok(AdminCustomerDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            code: state.custom_code.trim().to_string(),
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
            assigned_items,
        })
    }

    pub async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.read_port()?.items_page(query, limit, offset).await
    }

    pub async fn item_groups(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        let groups = self.read_port()?.item_groups(query, limit).await?;
        if groups.is_empty() && query.trim().is_empty() {
            Ok(vec!["All Item Groups".to_string()])
        } else {
            Ok(dedupe_strings(groups))
        }
    }

    pub async fn activity(
        &self,
        items: Option<AdminActivity>,
    ) -> Result<AdminActivity, AdminPortError> {
        Ok(items.unwrap_or_default().into_iter().take(30).collect())
    }

    fn read_port(&self) -> Result<&Arc<dyn AdminReadPort>, AdminPortError> {
        self.read_port.as_ref().ok_or(AdminPortError::LookupFailed)
    }

    async fn state_for(&self, ref_: &str) -> Result<AdminState, AdminPortError> {
        Ok(self
            .states()
            .await?
            .get(ref_.trim())
            .cloned()
            .unwrap_or_default())
    }

    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        match &self.state_port {
            Some(port) => port.states().await,
            None => Ok(BTreeMap::new()),
        }
    }

    fn admin_suppliers_from_entries(
        &self,
        entries: Vec<AdminDirectoryEntry>,
        states: &BTreeMap<String, AdminState>,
    ) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let mut result = Vec::with_capacity(entries.len());
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if state.removed {
                continue;
            }
            result.push(self.build_supplier(entry, state)?);
        }
        Ok(result)
    }

    fn build_supplier(
        &self,
        entry: AdminDirectoryEntry,
        state: AdminState,
    ) -> Result<AdminSupplier, AdminPortError> {
        let code = self.supplier_code(&entry, &state)?;
        Ok(AdminSupplier {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            code,
            blocked: state.blocked,
            removed: state.removed,
            assigned_item_count: state.assigned_item_codes.len(),
            assigned_item_codes: state.assigned_item_codes,
        })
    }

    fn supplier_code(
        &self,
        entry: &AdminDirectoryEntry,
        state: &AdminState,
    ) -> Result<String, AdminPortError> {
        let custom = state.custom_code.trim();
        if !custom.is_empty() {
            return Ok(custom.to_string());
        }
        supplier_access_code(&SupplierAccessInput {
            ref_: entry.ref_.clone(),
            name: entry.name.clone(),
            phone: entry.phone.clone(),
        })
        .map_err(|_| AdminPortError::LookupFailed)
    }
}

fn customer_directory_entry(entry: AdminDirectoryEntry) -> CustomerDirectoryEntry {
    CustomerDirectoryEntry {
        ref_: entry.ref_,
        name: entry.name,
        phone: entry.phone,
    }
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }
    result
}
