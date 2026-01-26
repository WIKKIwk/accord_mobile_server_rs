use std::sync::Arc;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::AppConfig;
use crate::core::auth::service::AuthService;
use crate::core::customer::service::CustomerService;
use crate::core::profile::service::ProfileService;
use crate::core::session::manager::SessionManager;
use crate::core::werka::service::WerkaService;
use crate::erpdb::reader::DirectDbReader;
use crate::erpnext::client::ErpnextClient;
use crate::store::admin_state_store::AdminSupplierStateStore;
use crate::store::profile_store::ProfileStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub auth: AuthService,
    pub customer: CustomerService,
    pub profiles: ProfileService,
    pub werka: WerkaService,
    pub sessions: SessionManager,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let mut auth = AuthService::new(&config);
        let mut customer = CustomerService::new();
        let profile_store = Arc::new(ProfileStore::new(config.profile_store_path.clone()));
        let mut profiles = ProfileService::new(config.erp_url.clone()).with_store(profile_store);
        let mut werka = WerkaService::new();
        let sessions = SessionManager::persistent(
            config.session_store_path.clone(),
            config.session_ttl_seconds,
        );
        let ai_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        if !ai_key.trim().is_empty() {
            werka = werka.with_ai_search(Arc::new(WerkaAiSearchService::new(
                &ai_key,
                &std::env::var("GEMINI_VISION_MODEL").unwrap_or_default(),
                config.erp_timeout,
            )));
        }

        if config.erp_configured() {
            let admin_state_store = Arc::new(AdminSupplierStateStore::new(
                config.admin_supplier_store_path.clone(),
            ));
            let erp_client = Arc::new(
                ErpnextClient::new(
                    config.erp_url.clone(),
                    config.erp_api_key.clone(),
                    config.erp_api_secret.clone(),
                    config.erp_timeout,
                )
                .with_default_warehouse(config.default_target_warehouse.clone()),
            );
            auth = auth.with_supplier_dependencies(erp_client.clone(), admin_state_store.clone());
            auth = auth.with_customer_dependencies(erp_client.clone(), admin_state_store.clone());
            customer = customer.with_delivery_port(erp_client.clone());
            profiles = profiles.with_erp_lookup(erp_client.clone());
            werka = werka
                .with_customer_issue_writer(erp_client.clone())
                .with_unannounced_writer(erp_client.clone())
                .with_supplier_unannounced_writer(erp_client.clone())
                .with_supplier_purchase_receipt_lookup(erp_client.clone())
                .with_supplier_item_lookup(erp_client.clone())
                .with_confirm_writer(erp_client.clone())
                .with_notification_detail_writer(erp_client.clone())
                .with_supplier_admin_state_lookup(admin_state_store);
        }
        match config.direct_db_config() {
            Ok(Some(db_config)) => {
                tracing::info!(
                    host = %db_config.host,
                    port = db_config.port,
                    database = %db_config.name,
                    "direct DB read enabled for Werka home"
                );
                let direct_reader = Arc::new(DirectDbReader::new(db_config));
                werka = werka
                    .with_lookup(direct_reader.clone())
                    .with_customer_issue_source_lookup(direct_reader.clone())
                    .with_notification_detail_lookup(direct_reader.clone())
                    .with_supplier_read_lookup(direct_reader);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "direct DB read disabled");
            }
        }

        Self {
            config: Arc::new(config),
            auth,
            customer,
            profiles,
            werka,
            sessions,
        }
    }
}
