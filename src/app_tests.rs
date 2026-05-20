use std::path::{Path, PathBuf};

use super::app_local_store::{LocalStoreBackend, derive_lmdb_path, local_store_backend_from};
use super::catalog_cache_sync_interval;

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
