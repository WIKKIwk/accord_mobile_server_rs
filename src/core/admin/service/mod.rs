mod helpers;
mod mutations;
mod read;

use std::collections::BTreeMap;
use std::sync::Arc;

use rand::Rng;
use time::OffsetDateTime;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::core::admin::models::{
    AdminActivity, AdminCustomerDetail, AdminDirectoryEntry, AdminItemGroupBulkMoveResult,
    AdminSettings, AdminState, AdminSupplier, AdminSupplierDetail, AdminSupplierSummary,
};
use crate::core::admin::ports::{
    AdminAuthConfigSink, AdminCredentialPort, AdminEnvPersister, AdminErpConfigSink,
    AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort,
};
use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::auth::models::Principal;
use crate::core::auth::service::normalize_phone;
use crate::core::authz::{
    Capability, MemoryRoleDefinitionStore, RoleAssignment, RoleAssignmentUpsert, RoleDefinition,
    RoleDefinitionStorePort, RoleDefinitionUpsert, capability_code, has_capability,
    normalize_custom_role, normalize_role_assignment, role_assignment_key, system_role_definitions,
};
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
    auth_config_sink: Option<Arc<dyn AdminAuthConfigSink>>,
    role_store: Arc<dyn RoleDefinitionStorePort>,
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
                werka_phone: config.werka_phone.clone(),
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
            auth_config_sink: None,
            role_store: Arc::new(MemoryRoleDefinitionStore::new()),
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

    pub fn with_auth_config_sink(mut self, auth_config_sink: Arc<dyn AdminAuthConfigSink>) -> Self {
        self.auth_config_sink = Some(auth_config_sink);
        self
    }

    pub fn with_role_store(mut self, role_store: Arc<dyn RoleDefinitionStorePort>) -> Self {
        self.role_store = role_store;
        self
    }

    pub async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, AdminPortError> {
        self.all_role_definitions().await
    }

    async fn all_role_definitions(&self) -> Result<Vec<RoleDefinition>, AdminPortError> {
        let mut roles = system_role_definitions();
        roles.extend(
            self.role_store
                .role_definitions()
                .await
                .map_err(|_| AdminPortError::LookupFailed)?,
        );
        roles.sort_by(|left, right| {
            left.system
                .cmp(&right.system)
                .reverse()
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(roles)
    }

    pub async fn role_assignments(&self) -> Result<Vec<RoleAssignment>, AdminPortError> {
        self.role_store
            .role_assignments()
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }

    pub async fn upsert_role_definition(
        &self,
        input: RoleDefinitionUpsert,
    ) -> Result<RoleDefinition, AdminPortError> {
        let role = normalize_custom_role(input)
            .map_err(|error| AdminPortError::InvalidInput(error.to_string()))?;
        self.role_store
            .put_role_definition(role.clone())
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(role)
    }

    pub async fn upsert_role_assignment(
        &self,
        input: RoleAssignmentUpsert,
    ) -> Result<RoleAssignment, AdminPortError> {
        let roles = self.all_role_definitions().await?;
        let assignment = normalize_role_assignment(input, &roles)
            .map_err(|error| AdminPortError::InvalidInput(error.to_string()))?;
        self.role_store
            .put_role_assignment(assignment.clone())
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(assignment)
    }

    pub async fn principal_has_capability(
        &self,
        principal: &Principal,
        capability: Capability,
    ) -> bool {
        match self.principal_assigned_role(principal).await {
            Ok(Some(role)) => capability_code(capability)
                .map(|code| role.capability_codes.iter().any(|item| item == code))
                .unwrap_or(false),
            Ok(None) => has_capability(principal, capability),
            Err(_) => false,
        }
    }

    async fn principal_assigned_role(
        &self,
        principal: &Principal,
    ) -> Result<Option<RoleDefinition>, AdminPortError> {
        let key = role_assignment_key(&principal.role, &principal.ref_);
        let Some(assignment) = self
            .role_assignments()
            .await?
            .into_iter()
            .find(|assignment| {
                role_assignment_key(&assignment.principal_role, &assignment.principal_ref) == key
            })
        else {
            return Ok(None);
        };
        self.all_role_definitions()
            .await?
            .into_iter()
            .find(|role| role.id == assignment.role_id)
            .map(Some)
            .ok_or(AdminPortError::LookupFailed)
    }

    pub async fn settings(&self) -> Result<AdminSettings, AdminPortError> {
        let config = self.config.read().await;
        let state = self.state_for("werka").await.unwrap_or_default();
        let now = OffsetDateTime::now_utc();
        let mut api_key = config.erp_api_key.clone();
        let mut api_secret = config.erp_api_secret.clone();
        if let Some(port) = &self.credential_port
            && let Ok((current_key, current_secret)) = port.admin_api_auth("Administrator").await
        {
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
        self.update_auth_runtime(
            &config.werka_phone,
            &config.werka_code,
            &config.werka_name,
            &config.admin_phone,
            &config.admin_name,
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

    fn update_auth_runtime(
        &self,
        werka_phone: &str,
        werka_code: &str,
        werka_name: &str,
        admin_phone: &str,
        admin_name: &str,
    ) {
        if let Some(sink) = &self.auth_config_sink {
            sink.set_runtime_identity(werka_phone, werka_code, werka_name, admin_phone, admin_name);
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

    async fn supplier_entries(
        &self,
        limit: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        const PAGE_SIZE: usize = 200;

        let read = self.read_port()?;
        let mut result = Vec::new();
        let mut offset = 0;
        loop {
            let mut page_limit = PAGE_SIZE;
            if limit > 0 {
                let remaining = limit.saturating_sub(result.len());
                if remaining == 0 {
                    break;
                }
                page_limit = page_limit.min(remaining);
            }
            let page = read.suppliers_page("", page_limit, offset).await?;
            let page_len = page.len();
            result.extend(page);
            if page_len < page_limit || (limit > 0 && result.len() >= limit) {
                break;
            }
            offset += page_limit;
        }
        if limit > 0 && result.len() > limit {
            result.truncate(limit);
        }
        Ok(result)
    }

    async fn supplier_entry_state(
        &self,
        ref_: &str,
        include_removed: bool,
    ) -> Result<(AdminDirectoryEntry, AdminState), AdminPortError> {
        let entry = self.read_port()?.supplier_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed && !include_removed {
            return Err(AdminPortError::NotFound);
        }
        Ok((entry, state))
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

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::config::AppConfig;
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::authz::{
        Capability, MemoryRoleDefinitionStore, RoleAssignment, RoleDefinitionStorePort,
    };

    use super::AdminService;

    #[tokio::test]
    async fn stale_role_assignment_does_not_fall_back_to_default_role_access() {
        let store = Arc::new(MemoryRoleDefinitionStore::new());
        store
            .put_role_assignment(RoleAssignment {
                principal_role: PrincipalRole::Werka,
                principal_ref: "werka".to_string(),
                role_id: "missing_role".to_string(),
            })
            .await
            .expect("put assignment");
        let service = AdminService::new(&test_config()).with_role_store(store);

        assert!(
            !service
                .principal_has_capability(
                    &principal(PrincipalRole::Werka, "werka"),
                    Capability::RpsBatchManage
                )
                .await
        );
    }

    fn principal(role: PrincipalRole, ref_: &str) -> Principal {
        Principal {
            role,
            display_name: "User".to_string(),
            legal_name: "User".to_string(),
            ref_: ref_.to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            erp_url: String::new(),
            erp_api_key: String::new(),
            erp_api_secret: String::new(),
            default_target_warehouse: String::new(),
            erp_timeout: Duration::from_secs(1),
            session_store_path: PathBuf::new(),
            profile_store_path: PathBuf::new(),
            push_token_store_path: PathBuf::new(),
            admin_supplier_store_path: PathBuf::new(),
            session_ttl_seconds: Some(3600),
            supplier_prefix: "10".to_string(),
            werka_prefix: "20".to_string(),
            werka_code: String::new(),
            werka_name: "Werka".to_string(),
            werka_phone: String::new(),
            admin_phone: String::new(),
            admin_name: "Admin".to_string(),
            admin_code: String::new(),
            direct_read_enabled: false,
            direct_site_config_path: String::new(),
            direct_db_host: String::new(),
            direct_db_port: None,
            direct_db_user: String::new(),
            direct_db_password: String::new(),
            direct_db_name: String::new(),
            catalog_cache_enabled: false,
            catalog_cache_fallback_direct_db: false,
            catalog_cache_path: PathBuf::new(),
        }
    }
}
