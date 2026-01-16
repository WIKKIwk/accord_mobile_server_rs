use std::sync::Arc;

use crate::config::AppConfig;
use crate::core::auth::service::AuthService;
use crate::core::session::manager::SessionManager;
use crate::erpnext::client::ErpnextClient;
use crate::store::admin_state_store::AdminSupplierStateStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub auth: AuthService,
    pub sessions: SessionManager,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let mut auth = AuthService::new(&config);
        let sessions = SessionManager::persistent(
            config.session_store_path.clone(),
            config.session_ttl_seconds,
        );

        if config.erp_configured() {
            auth = auth.with_supplier_dependencies(
                Arc::new(ErpnextClient::new(
                    config.erp_url.clone(),
                    config.erp_api_key.clone(),
                    config.erp_api_secret.clone(),
                    config.erp_timeout,
                )),
                Arc::new(AdminSupplierStateStore::new(
                    config.admin_supplier_store_path.clone(),
                )),
            );
        }

        Self {
            config: Arc::new(config),
            auth,
            sessions,
        }
    }
}
