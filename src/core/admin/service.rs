use std::collections::BTreeMap;
use std::sync::Arc;

use rand::Rng;
use time::OffsetDateTime;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::core::admin::models::{
    AdminActivity, AdminCustomerDetail, AdminDirectoryEntry, AdminItemGroupBulkMoveResult,
    AdminSettings, AdminState, AdminSupplier, AdminSupplierDetail, AdminSupplierSummary,
    AdminSuppliersPage,
};
use crate::core::admin::ports::{
    AdminCredentialPort, AdminEnvPersister, AdminErpConfigSink, AdminPortError, AdminReadPort,
    AdminStatePort, AdminWritePort,
};
use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::auth::service::normalize_phone;
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierItem};

const CODE_REGEN_WINDOW_SECONDS: i64 = 60;
const MAX_CODE_REGENS_PER_WINDOW: i32 = 3;

#[derive(Clone)]
pub struct AdminService {
    config: Arc<RwLock<AdminConfig>>,
    read_port: Option<Arc<dyn AdminReadPort>>,
    write_port: Option<Arc<dyn AdminWritePort>>,
    state_port: Option<Arc<dyn AdminStatePort>>,
    credential_port: Option<Arc<dyn AdminCredentialPort>>,
    env_persister: Option<Arc<dyn AdminEnvPersister>>,
    erp_config_sink: Option<Arc<dyn AdminErpConfigSink>>,
}

#[derive(Debug, Clone)]
struct AdminConfig {
    erp_url: String,
    erp_api_key: String,
    erp_api_secret: String,
    default_target_warehouse: String,
    default_uom: String,
    werka_phone: String,
    werka_name: String,
    werka_code: String,
    admin_phone: String,
    admin_name: String,
    supplier_prefix: String,
    werka_prefix: String,
}

impl AdminService {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(AdminConfig {
                erp_url: config.erp_url.clone(),
                erp_api_key: config.erp_api_key.clone(),
                erp_api_secret: config.erp_api_secret.clone(),
                default_target_warehouse: config.default_target_warehouse.clone(),
                default_uom: std::env::var("ERP_DEFAULT_UOM")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "Kg".to_string()),
                werka_phone: "+99888862440".to_string(),
                werka_name: config.werka_name.clone(),
                werka_code: config.werka_code.clone(),
                admin_phone: config.admin_phone.clone(),
                admin_name: config.admin_name.clone(),
                supplier_prefix: config.supplier_prefix.clone(),
                werka_prefix: config.werka_prefix.clone(),
            })),
            read_port: None,
            write_port: None,
            state_port: None,
            credential_port: None,
            env_persister: None,
            erp_config_sink: None,
        }
    }

    pub fn with_read_port(mut self, read_port: Arc<dyn AdminReadPort>) -> Self {
        self.read_port = Some(read_port);
        self
    }

    pub fn with_write_port(mut self, write_port: Arc<dyn AdminWritePort>) -> Self {
        self.write_port = Some(write_port);
        self
    }

    pub fn with_state_port(mut self, state_port: Arc<dyn AdminStatePort>) -> Self {
        self.state_port = Some(state_port);
        self
    }

    pub fn with_credential_port(mut self, credential_port: Arc<dyn AdminCredentialPort>) -> Self {
        self.credential_port = Some(credential_port);
        self
    }

    pub fn with_env_persister(mut self, env_persister: Arc<dyn AdminEnvPersister>) -> Self {
        self.env_persister = Some(env_persister);
        self
    }

    pub fn with_erp_config_sink(mut self, erp_config_sink: Arc<dyn AdminErpConfigSink>) -> Self {
        self.erp_config_sink = Some(erp_config_sink);
        self
    }

    pub async fn settings(&self) -> Result<AdminSettings, AdminPortError> {
        let config = self.config.read().await;
        let state = self.state_for("werka").await?;
        let now = OffsetDateTime::now_utc();
        let mut api_key = config.erp_api_key.clone();
        let mut api_secret = config.erp_api_secret.clone();
        if let Some(port) = &self.credential_port {
            if let Ok((current_key, current_secret)) = port.admin_api_auth("Administrator").await {
                if !current_key.trim().is_empty() {
                    api_key = current_key.trim().to_string();
                }
                if !current_secret.trim().is_empty() {
                    api_secret = current_secret.trim().to_string();
                }
                self.update_erp_runtime(
                    &config.erp_url,
                    &api_key,
                    &api_secret,
                    &config.default_target_warehouse,
                );
            }
        }
        Ok(AdminSettings {
            erp_url: config.erp_url.clone(),
            erp_api_key: api_key,
            erp_api_secret: api_secret,
            default_target_warehouse: config.default_target_warehouse.clone(),
            default_uom: config.default_uom.clone(),
            werka_phone: config.werka_phone.clone(),
            werka_name: config.werka_name.clone(),
            werka_code: config.werka_code.clone(),
            werka_code_locked: state.code_locked(now),
            werka_code_retry_after_sec: state.retry_after_seconds(now),
            admin_phone: config.admin_phone.clone(),
            admin_name: config.admin_name.clone(),
        })
    }

    pub async fn update_settings(
        &self,
        input: AdminSettings,
    ) -> Result<AdminSettings, AdminPortError> {
        let mut config = self.config.write().await;
        config.erp_url = input.erp_url.trim().to_string();
        config.erp_api_key = if input.erp_api_key.trim().is_empty() {
            config.erp_api_key.clone()
        } else {
            input.erp_api_key.trim().to_string()
        };
        config.erp_api_secret = if input.erp_api_secret.trim().is_empty() {
            config.erp_api_secret.clone()
        } else {
            input.erp_api_secret.trim().to_string()
        };
        config.default_target_warehouse = input.default_target_warehouse.trim().to_string();
        config.default_uom = input.default_uom.trim().to_string();
        if config.default_uom.is_empty() {
            config.default_uom = "Kg".to_string();
        }
        config.werka_phone = input.werka_phone.trim().to_string();
        config.werka_name = input.werka_name.trim().to_string();
        config.werka_code = input.werka_code.trim().to_string();
        config.admin_phone = input.admin_phone.trim().to_string();
        config.admin_name = input.admin_name.trim().to_string();
        if let Some(port) = &self.credential_port {
            let mut api_key = input.erp_api_key.trim().to_string();
            let mut api_secret = input.erp_api_secret.trim().to_string();
            if api_key.is_empty() || api_secret.is_empty() {
                let (current_key, current_secret) = port.admin_api_auth("Administrator").await?;
                if api_key.is_empty() {
                    api_key = current_key.trim().to_string();
                }
                if api_secret.is_empty() {
                    api_secret = current_secret.trim().to_string();
                }
            }
            port.update_admin_api_auth("Administrator", &api_key, &api_secret)
                .await?;
            config.erp_api_key = api_key;
            config.erp_api_secret = api_secret;
        }
        self.update_erp_runtime(
            &config.erp_url,
            &config.erp_api_key,
            &config.erp_api_secret,
            &config.default_target_warehouse,
        );
        if let Some(persister) = &self.env_persister {
            persister.upsert(BTreeMap::from([
                ("ERP_URL", config.erp_url.clone()),
                (
                    "ERP_DEFAULT_TARGET_WAREHOUSE",
                    config.default_target_warehouse.clone(),
                ),
                ("ERP_DEFAULT_UOM", config.default_uom.clone()),
                ("WERKA_PHONE", config.werka_phone.clone()),
                ("WERKA_NAME", config.werka_name.clone()),
                ("MOBILE_DEV_WERKA_CODE", config.werka_code.clone()),
                ("ADMINKA_PHONE", config.admin_phone.clone()),
                ("ADMINKA_NAME", config.admin_name.clone()),
            ]))?;
        }
        drop(config);
        self.settings().await
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

    pub async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminSupplier, AdminPortError> {
        let entry = self
            .write_port()?
            .create_supplier(name.trim(), phone.trim())
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        if state.removed {
            state.removed = false;
            state.blocked = false;
            self.put_state(&entry.ref_, state.clone()).await?;
        }
        self.build_supplier(entry, state)
    }

    pub async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<CustomerDirectoryEntry, AdminPortError> {
        self.write_port()?
            .create_customer(name.trim(), phone.trim())
            .await
            .map(customer_directory_entry)
    }

    pub async fn set_supplier_blocked(
        &self,
        ref_: &str,
        blocked: bool,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.blocked = blocked;
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn update_supplier_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let normalized = normalize_admin_phone(phone)?;
        self.write_port()?
            .update_supplier_phone(ref_.trim(), &normalized)
            .await?;
        self.supplier_detail(ref_).await
    }

    pub async fn update_customer_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let normalized = normalize_admin_phone(phone)?;
        self.write_port()?
            .update_customer_phone(ref_.trim(), &normalized)
            .await?;
        self.customer_detail(ref_).await
    }

    pub async fn update_supplier_items(
        &self,
        ref_: &str,
        item_codes: Vec<String>,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let normalized = normalize_item_codes(item_codes);
        if !normalized.is_empty() {
            let found = self.read_port()?.items_by_codes(&normalized).await?;
            for code in &normalized {
                if !found
                    .iter()
                    .any(|item| item.code.trim().eq_ignore_ascii_case(code.trim()))
                {
                    return Err(AdminPortError::InvalidInput(format!(
                        "item topilmadi: {code}"
                    )));
                }
            }
        }
        let current = self
            .read_port()?
            .assigned_supplier_items(&entry.ref_, 200)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.code)
            .collect::<Vec<_>>();
        for code in &normalized {
            if !current
                .iter()
                .any(|current| current.trim().eq_ignore_ascii_case(code))
            {
                self.write_port()?
                    .assign_supplier_item(&entry.ref_, code)
                    .await?;
            }
        }
        for code in current {
            if !normalized
                .iter()
                .any(|desired| desired.trim().eq_ignore_ascii_case(code.trim()))
            {
                self.write_port()?
                    .unassign_supplier_item(&entry.ref_, &code)
                    .await?;
            }
        }
        let mut state = self.state_for(&entry.ref_).await?;
        state.assignments_configured = true;
        state.assigned_item_codes = normalized;
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let code = item_code.trim();
        self.write_port()?
            .assign_supplier_item(&entry.ref_, code)
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.assignments_configured = true;
        state.assigned_item_codes = normalize_item_codes(
            state
                .assigned_item_codes
                .into_iter()
                .chain(std::iter::once(code.to_string()))
                .collect(),
        );
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        self.write_port()?
            .unassign_supplier_item(&entry.ref_, item_code.trim())
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.assignments_configured = true;
        state
            .assigned_item_codes
            .retain(|code| !code.trim().eq_ignore_ascii_case(item_code.trim()));
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        self.write_port()?
            .assign_customer_item(&entry.ref_, item_code.trim())
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    pub async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        self.write_port()?
            .unassign_customer_item(&entry.ref_, item_code.trim())
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    pub async fn regenerate_supplier_code(
        &self,
        ref_: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let mut existing = self.existing_codes().await?;
        let mut state = self.state_for(&entry.ref_).await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        state.custom_code = random_code(&self.config.read().await.supplier_prefix, &mut existing);
        state.pending_persist_code = state.custom_code.clone();
        state.pending_persist_at = Some(now + time::Duration::seconds(CODE_REGEN_WINDOW_SECONDS));
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn regenerate_customer_code(
        &self,
        ref_: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let mut existing = self.existing_state_codes().await?;
        let mut state = self.state_for(&entry.ref_).await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        state.custom_code = random_code("30", &mut existing);
        self.put_state(&entry.ref_, state.clone()).await?;
        self.write_port()?
            .update_customer_code(&entry.ref_, &state.custom_code)
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    pub async fn remove_supplier(&self, ref_: &str) -> Result<(), AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.removed = true;
        state.blocked = true;
        self.put_state(&entry.ref_, state).await
    }

    pub async fn restore_supplier(
        &self,
        ref_: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.removed = false;
        state.blocked = false;
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn remove_customer(&self, ref_: &str) -> Result<(), AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.removed = true;
        state.blocked = true;
        self.put_state(&entry.ref_, state).await
    }

    pub async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.write_port()?
            .create_item(code.trim(), name.trim(), uom.trim(), item_group.trim())
            .await
    }

    pub async fn move_items_to_group(
        &self,
        item_codes: Vec<String>,
        item_group: &str,
    ) -> Result<AdminItemGroupBulkMoveResult, AdminPortError> {
        let codes = normalize_item_codes(item_codes);
        if codes.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item codes are required".to_string(),
            ));
        }
        let group = item_group.trim();
        if group.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group is required".to_string(),
            ));
        }
        let mut updated = Vec::new();
        let mut failed = Vec::new();
        for code in &codes {
            if self
                .write_port()?
                .update_item_group(code, group)
                .await
                .is_ok()
            {
                updated.push(code.clone());
            } else {
                failed.push(code.clone());
            }
        }
        Ok(AdminItemGroupBulkMoveResult {
            item_group: group.to_string(),
            requested_count: codes.len(),
            updated_count: updated.len(),
            failed_count: failed.len(),
            updated_item_codes: updated,
            failed_item_codes: failed,
        })
    }

    pub async fn regenerate_werka_code(&self) -> Result<AdminSettings, AdminPortError> {
        let mut state = self.state_for("werka").await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        let mut existing = BTreeMap::new();
        let code = random_code(&self.config.read().await.werka_prefix, &mut existing);
        state.custom_code = code.clone();
        self.put_state("werka", state).await?;
        self.config.write().await.werka_code = code;
        if let Some(persister) = &self.env_persister {
            persister.upsert(BTreeMap::from([(
                "MOBILE_DEV_WERKA_CODE",
                self.config.read().await.werka_code.clone(),
            )]))?;
        }
        self.settings().await
    }

    fn read_port(&self) -> Result<&Arc<dyn AdminReadPort>, AdminPortError> {
        self.read_port.as_ref().ok_or(AdminPortError::LookupFailed)
    }

    fn write_port(&self) -> Result<&Arc<dyn AdminWritePort>, AdminPortError> {
        self.write_port.as_ref().ok_or(AdminPortError::LookupFailed)
    }

    fn update_erp_runtime(
        &self,
        base_url: &str,
        api_key: &str,
        api_secret: &str,
        default_warehouse: &str,
    ) {
        if let Some(sink) = &self.erp_config_sink {
            sink.set_erp_config(base_url, api_key, api_secret, default_warehouse);
        }
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

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.state_port
            .as_ref()
            .ok_or(AdminPortError::LookupFailed)?
            .put_state(ref_, state)
            .await
    }

    async fn existing_state_codes(&self) -> Result<BTreeMap<String, ()>, AdminPortError> {
        Ok(self
            .states()
            .await?
            .into_values()
            .filter_map(|state| {
                let code = state.custom_code.trim();
                (!code.is_empty()).then(|| (code.to_string(), ()))
            })
            .collect())
    }

    async fn existing_codes(&self) -> Result<BTreeMap<String, ()>, AdminPortError> {
        let states = self.states().await?;
        let entries = self.read_port()?.suppliers_page("", 0, 0).await?;
        let mut existing = BTreeMap::new();
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if state.removed {
                continue;
            }
            if let Ok(code) = self.supplier_code(&entry, &state) {
                existing.insert(code, ());
            }
        }
        Ok(existing)
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

fn normalize_item_codes(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            result.push(trimmed.to_string());
        }
    }
    result
}

fn normalize_admin_phone(phone: &str) -> Result<String, AdminPortError> {
    let mut clean = phone
        .replace(' ', "")
        .replace('-', "")
        .replace('(', "")
        .replace(')', "");
    if !clean.trim().starts_with('+') && clean.len() == 9 {
        clean = format!("998{clean}");
    }
    normalize_phone(&clean).map_err(|_| AdminPortError::LookupFailed)
}

fn bump_code_regen_state(
    mut state: AdminState,
    now: OffsetDateTime,
) -> Result<AdminState, AdminPortError> {
    if state.code_locked(now) {
        return Err(AdminPortError::CodeRegenCooldown);
    }
    if state
        .regen_window_started_at
        .map(|started| now - started >= time::Duration::seconds(CODE_REGEN_WINDOW_SECONDS))
        .unwrap_or(true)
    {
        state.regen_window_started_at = Some(now);
        state.regen_window_count = 0;
        state.cooldown_until = None;
    }
    state.regen_window_count += 1;
    if state.regen_window_count >= MAX_CODE_REGENS_PER_WINDOW {
        state.cooldown_until = state
            .regen_window_started_at
            .map(|started| started + time::Duration::seconds(CODE_REGEN_WINDOW_SECONDS));
    }
    Ok(state)
}

fn random_code(prefix: &str, existing: &mut BTreeMap<String, ()>) -> String {
    let prefix = if prefix.trim().is_empty() {
        "10"
    } else {
        prefix.trim()
    };
    loop {
        let suffix = (0..10)
            .map(|_| char::from(b'0' + rand::rng().random_range(0..10)))
            .collect::<String>();
        let code = format!("{prefix}{suffix}");
        if !existing.contains_key(&code) {
            existing.insert(code.clone(), ());
            return code;
        }
    }
}
