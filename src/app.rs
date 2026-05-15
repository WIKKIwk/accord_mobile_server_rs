use std::sync::Arc;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::{AppConfig, DotEnvPersister};
use crate::core::admin::service::AdminService;
use crate::core::auth::service::AuthService;
use crate::core::customer::service::CustomerService;
use crate::core::profile::ports::ProfileStorePort;
use crate::core::profile::service::ProfileService;
use crate::core::push::service::PushService;
use crate::core::session::manager::SessionManager;
use crate::core::werka::service::WerkaService;
use crate::erpdb::reader::DirectDbReader;
use crate::erpnext::client::ErpnextClient;
use crate::fcm::discover_push_sender;
use crate::store::admin_state_store::AdminSupplierStateStore;
use crate::store::profile_store::{LmdbProfileStore, ProfileStore};
use crate::store::push_token_store::PushTokenStore;

#[derive(Clone)]
pub struct AppState {
    #[cfg_attr(not(test), allow(dead_code))]
    pub config: Arc<AppConfig>,
    pub admin: AdminService,
    pub auth: AuthService,
    pub customer: CustomerService,
    pub profiles: ProfileService,
    pub push: PushService,
    pub werka: WerkaService,
    pub sessions: SessionManager,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let mut auth = AuthService::new(&config);
        let mut admin =
            AdminService::new(&config).with_env_persister(Arc::new(DotEnvPersister::new(".env")));
        admin = admin.with_auth_config_sink(Arc::new(auth.clone()));
        let mut customer = CustomerService::new();
        let profile_store = build_profile_store(&config);
        let push_token_store = Arc::new(PushTokenStore::new(config.push_token_store_path.clone()));
        let mut profiles = ProfileService::new(config.erp_url.clone()).with_store(profile_store);
        let push = PushService::new(push_token_store.clone())
            .with_sender(discover_push_sender(push_token_store));
        let mut werka = WerkaService::new();
        let session_store_backend =
            std::env::var("MOBILE_API_SESSION_STORE_BACKEND").unwrap_or_else(|_| "json".into());
        let sessions = match session_store_backend.trim().to_lowercase().as_str() {
            "lmdb" => match SessionManager::lmdb(
                session_lmdb_path(),
                session_lmdb_map_size_bytes(),
                config.session_ttl_seconds,
            ) {
                Ok(sessions) => {
                    tracing::info!(
                        path = %session_lmdb_path().display(),
                        "LMDB session store enabled"
                    );
                    sessions
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "LMDB session store unavailable; falling back to JSON session store"
                    );
                    SessionManager::persistent(
                        config.session_store_path.clone(),
                        config.session_ttl_seconds,
                    )
                }
            },
            _ => SessionManager::persistent(
                config.session_store_path.clone(),
                config.session_ttl_seconds,
            ),
        };
        let ai_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        if !ai_key.trim().is_empty() {
            werka = werka.with_ai_search(Arc::new(WerkaAiSearchService::new(
                &ai_key,
                &std::env::var("GEMINI_VISION_MODEL").unwrap_or_default(),
                config.erp_timeout,
            )));
        }

        let mut erp_client = None;
        if config.erp_configured() {
            let admin_state_store = Arc::new(AdminSupplierStateStore::new(
                config.admin_supplier_store_path.clone(),
            ));
            let client = Arc::new(
                ErpnextClient::new(
                    config.erp_url.clone(),
                    config.erp_api_key.clone(),
                    config.erp_api_secret.clone(),
                    config.erp_timeout,
                )
                .with_default_warehouse(config.default_target_warehouse.clone()),
            );
            admin = admin
                .with_read_port(client.clone())
                .with_write_port(client.clone())
                .with_erp_config_sink(client.clone())
                .with_state_port(admin_state_store.clone());
            auth = auth.with_supplier_dependencies(client.clone(), admin_state_store.clone());
            auth = auth.with_customer_dependencies(client.clone(), admin_state_store.clone());
            customer = customer.with_delivery_port(client.clone());
            profiles = profiles.with_erp_lookup(client.clone());
            werka = werka
                .with_customer_issue_writer(client.clone())
                .with_unannounced_writer(client.clone())
                .with_supplier_unannounced_writer(client.clone())
                .with_supplier_purchase_receipt_lookup(client.clone())
                .with_supplier_item_lookup(client.clone())
                .with_confirm_writer(client.clone())
                .with_notification_detail_writer(client.clone())
                .with_supplier_admin_state_lookup(admin_state_store);
            erp_client = Some(client);
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
                if let Some(client) = &erp_client {
                    client.set_credential_provider(direct_reader.clone());
                }
                admin = admin
                    .with_read_port(direct_reader.clone())
                    .with_credential_port(direct_reader.clone());
                werka = werka
                    .with_lookup(direct_reader.clone())
                    .with_customer_issue_source_lookup(direct_reader.clone())
                    .with_notification_detail_lookup(direct_reader.clone())
                    .with_supplier_read_lookup(direct_reader.clone());
                profiles = profiles.with_read_lookup(direct_reader.clone());
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "direct DB read disabled");
            }
        }

        Self {
            config: Arc::new(config),
            admin,
            auth,
            customer,
            profiles,
            push,
            werka,
            sessions,
        }
    }
}

fn session_lmdb_path() -> std::path::PathBuf {
    std::env::var("MOBILE_API_SESSION_LMDB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data/mobile_sessions.lmdb"))
}

fn session_lmdb_map_size_bytes() -> usize {
    std::env::var("MOBILE_API_SESSION_LMDB_MAP_SIZE_MB")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(64)
        * 1024
        * 1024
}

fn build_profile_store(config: &AppConfig) -> Arc<dyn ProfileStorePort> {
    let backend =
        std::env::var("MOBILE_API_PROFILE_STORE_BACKEND").unwrap_or_else(|_| "json".into());
    match backend.trim().to_lowercase().as_str() {
        "lmdb" => match LmdbProfileStore::open(
            profile_lmdb_path(),
            profile_lmdb_map_size_bytes(),
            Some(config.profile_store_path.clone()),
        ) {
            Ok(store) => {
                tracing::info!(
                    path = %profile_lmdb_path().display(),
                    legacy_json_path = %config.profile_store_path.display(),
                    "LMDB profile preference store enabled"
                );
                Arc::new(store)
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "LMDB profile preference store unavailable; falling back to JSON profile store"
                );
                Arc::new(ProfileStore::new(config.profile_store_path.clone()))
            }
        },
        _ => Arc::new(ProfileStore::new(config.profile_store_path.clone())),
    }
}

fn profile_lmdb_path() -> std::path::PathBuf {
    std::env::var("MOBILE_API_PROFILE_LMDB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("data/mobile_profile_prefs.lmdb"))
}

fn profile_lmdb_map_size_bytes() -> usize {
    std::env::var("MOBILE_API_PROFILE_LMDB_MAP_SIZE_MB")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(64)
        * 1024
        * 1024
}
