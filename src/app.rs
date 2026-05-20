use std::sync::Arc;
use std::time::Duration;

use crate::ai::werka_search::WerkaAiSearchService;
use crate::config::{AppConfig, DotEnvPersister};
use crate::core::admin::service::AdminService;
use crate::core::auth::service::AuthService;
use crate::core::customer::service::CustomerService;
use crate::core::gscale::GscaleService;
use crate::core::profile::ports::ProfileStorePort;
use crate::core::profile::service::ProfileService;
use crate::core::push::ports::PushTokenStorePort;
use crate::core::push::service::PushService;
use crate::core::rps_batch::{RpsBatchLmdbStore, RpsBatchService};
use crate::core::session::manager::SessionManager;
use crate::core::werka::service::WerkaService;
use crate::erpdb::catalog_cache::reader::CatalogCacheReader;
use crate::erpdb::catalog_cache::store::CatalogCacheStore;
use crate::erpdb::catalog_cache::sync::{sync_catalog_delta_once, sync_catalog_once};
use crate::erpdb::reader::DirectDbReader;
use crate::erpnext::client::ErpnextClient;
use crate::fcm::discover_push_sender;
use crate::rps::RpsDriverClient;
use crate::store::admin_state_store::AdminSupplierStateBackend;
use crate::store::profile_store::{LmdbProfileStore, ProfileStore};
use crate::store::push_token_store::{LmdbPushTokenStore, PushTokenStore};
use tokio::time::sleep;

#[derive(Clone)]
pub struct AppState {
    #[cfg_attr(not(test), allow(dead_code))]
    pub config: Arc<AppConfig>,
    pub admin: AdminService,
    pub auth: AuthService,
    pub customer: CustomerService,
    pub profiles: ProfileService,
    pub push: PushService,
    pub gscale: GscaleService,
    pub rps_batch: RpsBatchService,
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
        let push_token_store = build_push_token_store(&config);
        let mut profiles = ProfileService::new(config.erp_url.clone()).with_store(profile_store);
        let push = PushService::new(push_token_store.clone())
            .with_sender(discover_push_sender(push_token_store));
        let rps_batch = RpsBatchService::new(Arc::new(build_rps_batch_store()));
        let mut gscale = GscaleService::new().with_driver(Arc::new(RpsDriverClient::new(
            config.erp_timeout,
            std::env::var("RP_SCALE_DRIVER_URL").unwrap_or_default(),
        )));
        let mut werka = WerkaService::new();
        let sessions = match local_store_backend("MOBILE_API_SESSION_STORE_BACKEND") {
            LocalStoreBackend::Lmdb => {
                let lmdb_path = session_lmdb_path(&config);
                match SessionManager::lmdb(
                    lmdb_path.clone(),
                    local_lmdb_map_size_bytes("MOBILE_API_SESSION_LMDB_MAP_SIZE_MB"),
                    config.session_ttl_seconds,
                ) {
                    Ok(sessions) => {
                        tracing::info!(
                            path = %lmdb_path.display(),
                            "LMDB session store enabled"
                        );
                        sessions
                    }
                    Err(error) => {
                        if allow_json_fallback() {
                            tracing::warn!(
                                %error,
                                "LMDB session store unavailable; falling back to JSON session store"
                            );
                            SessionManager::persistent(
                                config.session_store_path.clone(),
                                config.session_ttl_seconds,
                            )
                        } else {
                            panic!("LMDB session store unavailable: {error}");
                        }
                    }
                }
            }
            LocalStoreBackend::Json => SessionManager::persistent(
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
            let admin_state_store = Arc::new(build_admin_supplier_state_store(&config));
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
            gscale = gscale.with_erp(client.clone());
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
                let direct_reader = Arc::new(DirectDbReader::new(db_config.clone()));
                if let Some(client) = &erp_client {
                    client.set_credential_provider(direct_reader.clone());
                }
                let catalog_reader = if config.catalog_cache_enabled {
                    match CatalogCacheStore::open(&config.catalog_cache_path) {
                        Ok(store) => {
                            let store = Arc::new(store);
                            let sync_store = store.clone();
                            let sync_reader = (*direct_reader).clone();
                            let sync_interval = catalog_cache_sync_interval();
                            tokio::spawn(async move {
                                run_catalog_cache_sync_loop(sync_reader, sync_store, sync_interval)
                                    .await;
                            });
                            Some(Arc::new(
                                CatalogCacheReader::new(store, db_config.default_warehouse.clone())
                                    .with_fallback(direct_reader.clone()),
                            ))
                        }
                        Err(error) => {
                            if config.catalog_cache_fallback_direct_db {
                                tracing::warn!(
                                    %error,
                                    path = %config.catalog_cache_path.display(),
                                    "catalog cache unavailable; using direct DB reads"
                                );
                                None
                            } else {
                                panic!("catalog cache unavailable: {error}");
                            }
                        }
                    }
                } else {
                    None
                };

                if let Some(catalog_reader) = catalog_reader {
                    admin = admin
                        .with_read_port(catalog_reader.clone())
                        .with_credential_port(direct_reader.clone());
                    werka = werka
                        .with_lookup(catalog_reader.clone())
                        .with_customer_issue_source_lookup(direct_reader.clone())
                        .with_notification_detail_lookup(direct_reader.clone())
                        .with_supplier_read_lookup(direct_reader.clone());
                    profiles = profiles.with_read_lookup(catalog_reader.clone());
                } else {
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
            gscale,
            rps_batch,
            werka,
            sessions,
        }
    }
}

async fn run_catalog_cache_sync_loop(
    direct_reader: DirectDbReader,
    store: Arc<CatalogCacheStore>,
    interval: Duration,
) {
    let mut full_sync_needed = true;
    loop {
        let was_full_sync = full_sync_needed;
        let sync_result = if full_sync_needed {
            sync_catalog_once(&direct_reader, &store).await
        } else {
            sync_catalog_delta_once(&direct_reader, &store).await
        };

        match sync_result {
            Ok(report) => {
                full_sync_needed = false;
                tracing::info!(
                    full = was_full_sync,
                    items = report.items,
                    item_groups = report.item_groups,
                    suppliers = report.suppliers,
                    customers = report.customers,
                    item_suppliers = report.item_suppliers,
                    item_customers = report.item_customers,
                    "catalog cache sync completed"
                );
            }
            Err(error) => {
                tracing::warn!(%error, "catalog cache sync failed");
                full_sync_needed = true;
            }
        }
        if interval.is_zero() {
            break;
        }
        sleep(interval).await;
    }
}

fn catalog_cache_sync_interval() -> Duration {
    let ms = std::env::var("ERP_CATALOG_CACHE_SYNC_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(1_000);
    Duration::from_millis(ms)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalStoreBackend {
    Lmdb,
    Json,
}

fn local_store_backend(env_key: &'static str) -> LocalStoreBackend {
    let raw = std::env::var(env_key).ok();
    let backend = local_store_backend_from(raw.as_deref());
    if raw.is_some()
        && !matches!(
            raw.as_deref().map(|value| value.trim().to_lowercase()),
            Some(value) if value == "lmdb" || value == "json"
        )
    {
        tracing::warn!(
            env_key,
            value = %raw.as_deref().unwrap_or_default(),
            "unknown local store backend; using LMDB"
        );
    }
    backend
}

fn local_store_backend_from(raw: Option<&str>) -> LocalStoreBackend {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("json") => LocalStoreBackend::Json,
        _ => LocalStoreBackend::Lmdb,
    }
}

fn session_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_SESSION_LMDB_PATH",
        &config.session_store_path,
        "data/mobile_sessions.lmdb",
    )
}

fn local_lmdb_map_size_bytes(env_key: &str) -> usize {
    std::env::var(env_key)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(64)
        * 1024
        * 1024
}

fn build_profile_store(config: &AppConfig) -> Arc<dyn ProfileStorePort> {
    match local_store_backend("MOBILE_API_PROFILE_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = profile_lmdb_path(config);
            match LmdbProfileStore::open(
                lmdb_path.clone(),
                local_lmdb_map_size_bytes("MOBILE_API_PROFILE_LMDB_MAP_SIZE_MB"),
                Some(config.profile_store_path.clone()),
            ) {
                Ok(store) => {
                    tracing::info!(
                        path = %lmdb_path.display(),
                        legacy_json_path = %config.profile_store_path.display(),
                        "LMDB profile preference store enabled"
                    );
                    Arc::new(store)
                }
                Err(error) => {
                    if allow_json_fallback() {
                        tracing::warn!(
                            %error,
                            "LMDB profile preference store unavailable; falling back to JSON profile store"
                        );
                        Arc::new(ProfileStore::new(config.profile_store_path.clone()))
                    } else {
                        panic!("LMDB profile preference store unavailable: {error}");
                    }
                }
            }
        }
        LocalStoreBackend::Json => Arc::new(ProfileStore::new(config.profile_store_path.clone())),
    }
}

fn profile_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_PROFILE_LMDB_PATH",
        &config.profile_store_path,
        "data/mobile_profile_prefs.lmdb",
    )
}

fn build_push_token_store(config: &AppConfig) -> Arc<dyn PushTokenStorePort> {
    match local_store_backend("MOBILE_API_PUSH_TOKEN_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = push_token_lmdb_path(config);
            match LmdbPushTokenStore::open(
                lmdb_path.clone(),
                local_lmdb_map_size_bytes("MOBILE_API_PUSH_TOKEN_LMDB_MAP_SIZE_MB"),
                Some(config.push_token_store_path.clone()),
            ) {
                Ok(store) => {
                    tracing::info!(
                        path = %lmdb_path.display(),
                        legacy_json_path = %config.push_token_store_path.display(),
                        "LMDB push token store enabled"
                    );
                    Arc::new(store)
                }
                Err(error) => {
                    if allow_json_fallback() {
                        tracing::warn!(
                            %error,
                            "LMDB push token store unavailable; falling back to JSON push token store"
                        );
                        Arc::new(PushTokenStore::new(config.push_token_store_path.clone()))
                    } else {
                        panic!("LMDB push token store unavailable: {error}");
                    }
                }
            }
        }
        LocalStoreBackend::Json => {
            Arc::new(PushTokenStore::new(config.push_token_store_path.clone()))
        }
    }
}

fn push_token_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_PUSH_TOKEN_LMDB_PATH",
        &config.push_token_store_path,
        "data/mobile_push_tokens.lmdb",
    )
}

fn build_admin_supplier_state_store(config: &AppConfig) -> AdminSupplierStateBackend {
    match local_store_backend("MOBILE_API_ADMIN_SUPPLIER_STORE_BACKEND") {
        LocalStoreBackend::Lmdb => {
            let lmdb_path = admin_supplier_lmdb_path(config);
            match AdminSupplierStateBackend::lmdb(
                lmdb_path.clone(),
                local_lmdb_map_size_bytes("MOBILE_API_ADMIN_SUPPLIER_LMDB_MAP_SIZE_MB"),
                Some(config.admin_supplier_store_path.clone()),
            ) {
                Ok(store) => {
                    tracing::info!(
                        path = %lmdb_path.display(),
                        legacy_json_path = %config.admin_supplier_store_path.display(),
                        "LMDB admin supplier state store enabled"
                    );
                    store
                }
                Err(error) => {
                    if allow_json_fallback() {
                        tracing::warn!(
                            %error,
                            "LMDB admin supplier state store unavailable; falling back to JSON admin supplier state store"
                        );
                        AdminSupplierStateBackend::json(config.admin_supplier_store_path.clone())
                    } else {
                        panic!("LMDB admin supplier state store unavailable: {error}");
                    }
                }
            }
        }
        LocalStoreBackend::Json => {
            AdminSupplierStateBackend::json(config.admin_supplier_store_path.clone())
        }
    }
}

fn admin_supplier_lmdb_path(config: &AppConfig) -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_ADMIN_SUPPLIER_LMDB_PATH",
        &config.admin_supplier_store_path,
        "data/mobile_admin_suppliers.lmdb",
    )
}

fn build_rps_batch_store() -> RpsBatchLmdbStore {
    let lmdb_path = rps_batch_lmdb_path();
    match RpsBatchLmdbStore::open(
        lmdb_path.clone(),
        local_lmdb_map_size_bytes("MOBILE_API_RPS_BATCH_LMDB_MAP_SIZE_MB"),
    ) {
        Ok(store) => {
            tracing::info!(
                path = %lmdb_path.display(),
                "LMDB RPS batch store enabled"
            );
            store
        }
        Err(error) => panic!("LMDB RPS batch store unavailable: {error}"),
    }
}

fn rps_batch_lmdb_path() -> std::path::PathBuf {
    lmdb_path(
        "MOBILE_API_RPS_BATCH_LMDB_PATH",
        std::path::Path::new("data/mobile_rps_batches.json"),
        "data/mobile_rps_batches.lmdb",
    )
}

fn lmdb_path(
    env_key: &str,
    legacy_json_path: &std::path::Path,
    hardcoded_default: &str,
) -> std::path::PathBuf {
    match std::env::var(env_key) {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            #[cfg(test)]
            {
                test_lmdb_path(legacy_json_path, hardcoded_default)
            }
            #[cfg(not(test))]
            {
                derive_lmdb_path(legacy_json_path, hardcoded_default)
            }
        }
    }
}

fn derive_lmdb_path(
    legacy_json_path: &std::path::Path,
    hardcoded_default: &str,
) -> std::path::PathBuf {
    if legacy_json_path.as_os_str().is_empty() {
        return std::path::PathBuf::from(hardcoded_default);
    }
    legacy_json_path.with_extension("lmdb")
}

fn allow_json_fallback() -> bool {
    std::env::var("MOBILE_API_LOCAL_STORE_ALLOW_JSON_FALLBACK").is_ok_and(|raw| raw.trim() == "1")
}

#[cfg(test)]
fn test_lmdb_path(
    legacy_json_path: &std::path::Path,
    hardcoded_default: &str,
) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let stem = legacy_json_path
        .file_stem()
        .or_else(|| std::path::Path::new(hardcoded_default).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("local-store");
    std::env::temp_dir().join(format!(
        "accord-mobile-server-rs-lmdb-test-{}-{count}-{stem}.lmdb",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        LocalStoreBackend, catalog_cache_sync_interval, derive_lmdb_path, local_store_backend_from,
    };

    #[test]
    fn local_store_backend_defaults_to_lmdb_for_production() {
        assert_eq!(local_store_backend_from(None), LocalStoreBackend::Lmdb);
        assert_eq!(local_store_backend_from(Some("")), LocalStoreBackend::Lmdb);
        assert_eq!(
            local_store_backend_from(Some("unknown")),
            LocalStoreBackend::Lmdb
        );
    }

    #[test]
    fn local_store_backend_accepts_explicit_json_and_lmdb() {
        assert_eq!(
            local_store_backend_from(Some("json")),
            LocalStoreBackend::Json
        );
        assert_eq!(
            local_store_backend_from(Some(" JSON ")),
            LocalStoreBackend::Json
        );
        assert_eq!(
            local_store_backend_from(Some("lmdb")),
            LocalStoreBackend::Lmdb
        );
        assert_eq!(
            local_store_backend_from(Some(" LMDB ")),
            LocalStoreBackend::Lmdb
        );
    }

    #[test]
    fn lmdb_path_defaults_next_to_legacy_json_path() {
        assert_eq!(
            derive_lmdb_path(Path::new("data/mobile_sessions.json"), "fallback.lmdb"),
            PathBuf::from("data/mobile_sessions.lmdb")
        );
        assert_eq!(
            derive_lmdb_path(Path::new(""), "fallback.lmdb"),
            PathBuf::from("fallback.lmdb")
        );
    }

    #[test]
    fn catalog_cache_sync_interval_defaults_to_one_second() {
        unsafe {
            std::env::remove_var("ERP_CATALOG_CACHE_SYNC_INTERVAL_MS");
        }

        assert_eq!(
            catalog_cache_sync_interval(),
            std::time::Duration::from_secs(1)
        );
    }
}
