# Catalog Read Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a feature-flagged SQLite catalog read cache for ERPNext catalog data used by the Rust mobile server.

**Architecture:** Keep MariaDB/ERPNext as source of truth. Add a small SQLite cache module that can sync catalog tables from the existing direct DB connection and serve existing read ports without changing HTTP/mobile contracts. Keep `DirectDbReader` as fallback.

**Tech Stack:** Rust 2024, Tokio, sqlx MySQL, rusqlite SQLite, existing async traits and Axum services.

---

## File Structure

- Modify `Cargo.toml`: add SQLite dependency.
- Modify `src/config.rs`: add cache feature flags and path.
- Modify `src/app.rs`: wire cache reader when enabled.
- Modify `src/erpdb/mod.rs`: expose cache module.
- Create `src/erpdb/catalog_cache/mod.rs`: module boundary and public types.
- Create `src/erpdb/catalog_cache/schema.rs`: SQLite schema migration.
- Create `src/erpdb/catalog_cache/store.rs`: SQLite open, upsert, and query functions.
- Create `src/erpdb/catalog_cache/sync.rs`: MariaDB-to-SQLite sync using direct DB config.
- Create `src/erpdb/catalog_cache/reader.rs`: read adapter implementing existing ports.
- Create tests beside new module files.

## Task 1: Add Cache Configuration

**Files:**
- Modify: `src/config.rs`
- Test: `src/config/tests.rs`

- [ ] **Step 1: Write failing config tests**

Add tests that verify:

```rust
#[test]
fn catalog_cache_config_defaults_disabled() {
    temp_env::with_vars(
        [
            ("ERP_CATALOG_CACHE_ENABLED", None::<&str>),
            ("ERP_CATALOG_CACHE_FALLBACK_DIRECT_DB", None::<&str>),
            ("ERP_CATALOG_CACHE_PATH", None::<&str>),
        ],
        || {
            let config = AppConfig::from_env();
            assert!(!config.catalog_cache_enabled);
            assert!(config.catalog_cache_fallback_direct_db);
            assert_eq!(config.catalog_cache_path, "data/catalog_cache.sqlite");
        },
    );
}

#[test]
fn catalog_cache_config_reads_env() {
    temp_env::with_vars(
        [
            ("ERP_CATALOG_CACHE_ENABLED", Some("1")),
            ("ERP_CATALOG_CACHE_FALLBACK_DIRECT_DB", Some("0")),
            ("ERP_CATALOG_CACHE_PATH", Some("/tmp/catalog.sqlite")),
        ],
        || {
            let config = AppConfig::from_env();
            assert!(config.catalog_cache_enabled);
            assert!(!config.catalog_cache_fallback_direct_db);
            assert_eq!(config.catalog_cache_path, "/tmp/catalog.sqlite");
        },
    );
}
```

- [ ] **Step 2: Run config tests**

Run: `cargo test --locked config::tests::catalog_cache -- --nocapture`

Expected: fail because fields do not exist.

- [ ] **Step 3: Implement config fields**

Add to `AppConfig`:

```rust
pub catalog_cache_enabled: bool,
pub catalog_cache_fallback_direct_db: bool,
pub catalog_cache_path: String,
```

Populate them from env:

```rust
catalog_cache_enabled: env_or("ERP_CATALOG_CACHE_ENABLED", "") == "1",
catalog_cache_fallback_direct_db: env_or("ERP_CATALOG_CACHE_FALLBACK_DIRECT_DB", "1") != "0",
catalog_cache_path: env_or("ERP_CATALOG_CACHE_PATH", "data/catalog_cache.sqlite"),
```

- [ ] **Step 4: Run config tests again**

Run: `cargo test --locked config::tests::catalog_cache -- --nocapture`

Expected: pass.

## Task 2: Add SQLite Schema

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/erpdb/mod.rs`
- Create: `src/erpdb/catalog_cache/mod.rs`
- Create: `src/erpdb/catalog_cache/schema.rs`
- Test: `src/erpdb/catalog_cache/schema.rs`

- [ ] **Step 1: Add dependency**

Add:

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```

- [ ] **Step 2: Write schema migration test**

Create an in-memory SQLite connection, call `migrate`, then assert all expected tables exist.

Run: `cargo test --locked erpdb::catalog_cache::schema -- --nocapture`

Expected: fail until schema exists.

- [ ] **Step 3: Implement schema**

Create tables:

- `catalog_items`
- `catalog_item_groups`
- `catalog_suppliers`
- `catalog_customers`
- `catalog_item_suppliers`
- `catalog_item_customers`
- `catalog_sync_state`

Create indexes:

- `idx_catalog_items_name`
- `idx_catalog_items_item_name`
- `idx_catalog_items_group`
- `idx_catalog_item_groups_lft`
- `idx_catalog_item_suppliers_supplier`
- `idx_catalog_item_customers_customer`

- [ ] **Step 4: Run schema tests**

Run: `cargo test --locked erpdb::catalog_cache::schema -- --nocapture`

Expected: pass.

## Task 3: Add SQLite Store Queries

**Files:**
- Create: `src/erpdb/catalog_cache/store.rs`
- Modify: `src/erpdb/catalog_cache/mod.rs`
- Test: `src/erpdb/catalog_cache/store.rs`

- [ ] **Step 1: Write store tests**

Cover:

- upsert item then search by code;
- upsert item then search by name;
- limit/offset pagination;
- group filter;
- supplier mapping;
- customer mapping;
- item group tree ordering by `lft`.

- [ ] **Step 2: Run store tests**

Run: `cargo test --locked erpdb::catalog_cache::store -- --nocapture`

Expected: fail until store exists.

- [ ] **Step 3: Implement store**

Implement `CatalogCacheStore` with:

```rust
pub fn open(path: impl AsRef<Path>) -> Result<Self, CatalogCacheError>;
pub fn in_memory() -> Result<Self, CatalogCacheError>;
pub fn upsert_items(&self, items: &[CachedItem]) -> Result<(), CatalogCacheError>;
pub fn items_page(&self, query: &str, group: Option<&str>, limit: usize, offset: usize) -> Result<Vec<SupplierItem>, CatalogCacheError>;
pub fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, CatalogCacheError>;
pub fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, CatalogCacheError>;
```

Add supplier/customer equivalents after the item path passes.

- [ ] **Step 4: Run store tests**

Run: `cargo test --locked erpdb::catalog_cache::store -- --nocapture`

Expected: pass.

## Task 4: Add Direct DB Sync

**Files:**
- Create: `src/erpdb/catalog_cache/sync.rs`
- Modify: `src/erpdb/catalog_cache/mod.rs`
- Test: `src/erpdb/catalog_cache/sync.rs`

- [ ] **Step 1: Write sync mapper tests**

Use Rust structs that represent direct DB rows and assert they map into cache rows correctly.

- [ ] **Step 2: Implement full sync**

Implement a first-pass full sync:

```rust
pub async fn sync_catalog_once(
    direct: &DirectDbReader,
    store: &CatalogCacheStore,
) -> Result<CatalogSyncReport, CatalogCacheError>;
```

The first version reads the six catalog tables and upserts into SQLite.

- [ ] **Step 3: Run sync tests**

Run: `cargo test --locked erpdb::catalog_cache::sync -- --nocapture`

Expected: pass.

## Task 5: Add Cached Reader Adapter

**Files:**
- Create: `src/erpdb/catalog_cache/reader.rs`
- Modify: `src/erpdb/catalog_cache/mod.rs`
- Test: `src/erpdb/catalog_cache/reader.rs`

- [ ] **Step 1: Write adapter tests**

Assert that the adapter implements and returns correct values for:

- `AdminReadPort::items_page`
- `AdminReadPort::items_page_by_group`
- `AdminReadPort::item_group_tree`
- `WerkaHomeLookup::werka_supplier_items`
- `WerkaHomeLookup::werka_customer_item_options`
- `ProfileLookup::get_supplier_profile`

- [ ] **Step 2: Implement adapter**

Create:

```rust
pub struct CatalogCacheReader {
    store: Arc<CatalogCacheStore>,
    fallback: Option<Arc<DirectDbReader>>,
}
```

Map SQLite errors to existing `AdminPortError`, `WerkaPortError`, and `ProfilePortError`.

- [ ] **Step 3: Run adapter tests**

Run: `cargo test --locked erpdb::catalog_cache::reader -- --nocapture`

Expected: pass.

## Task 6: Wire App State

**Files:**
- Modify: `src/app.rs`
- Test: existing route tests plus config tests.

- [ ] **Step 1: Wire only when enabled**

When `catalog_cache_enabled` is true and direct DB config exists:

1. Create `DirectDbReader`.
2. Create/open `CatalogCacheStore`.
3. Run `sync_catalog_once`.
4. Build `CatalogCacheReader`.
5. Use it for catalog read ports.

Keep direct DB reader for operational read ports and credentials.

- [ ] **Step 2: Keep fallback path**

If cache setup fails and `catalog_cache_fallback_direct_db` is true, log and keep current `DirectDbReader` wiring.

If fallback is false, panic during startup with exact error.

- [ ] **Step 3: Run focused tests**

Run:

```bash
cargo test --locked erpdb::catalog_cache -- --nocapture
cargo test --locked http::admin_route_tests http::werka_items_route_tests http::werka_directory_route_tests -- --nocapture
```

Expected: pass.

## Task 7: Real ERPNext Verification

**Files:**
- No committed code unless test helper is needed.

- [ ] **Step 1: Run direct DB vs cache comparison locally**

Use the local ERPNext path `/Volumes/Samsung990P/local.git/erpnext_n1/erp` only for test data/config discovery. Do not mutate ERPNext data for this comparison.

- [ ] **Step 2: Compare outputs**

Compare:

- first 80 items;
- search for a known item;
- item groups;
- supplier list;
- customer list;
- supplier item list;
- customer item options.

- [ ] **Step 3: Record result**

If useful, add a short benchmark note under `docs/benchmarks/`.

## Task 8: Final Verification And Commit

**Files:**
- All modified implementation files.

- [ ] **Step 1: Format**

Run: `cargo fmt --check`

Expected: pass.

- [ ] **Step 2: Full tests**

Run: `cargo test --locked`

Expected: pass.

- [ ] **Step 3: Commit**

Run:

```bash
git add Cargo.toml Cargo.lock src docs
git commit -m "Add catalog read cache"
```

Use real current date for this repository.
