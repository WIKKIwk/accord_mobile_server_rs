# Catalog Read Cache Design

## Goal

Move high-frequency ERPNext catalog reads from MariaDB direct queries to a local SQLite read cache inside the Rust server, while keeping ERPNext/MariaDB as the source of truth.

## Scope

This first phase caches only low-risk catalog data:

- `tabItem`
- `tabItem Group`
- `tabSupplier`
- `tabCustomer`
- `tabItem Supplier`
- `tabItem Customer Detail`

The cache serves item search, admin item lists, item group picker/tree, supplier/customer directories, supplier item lists, and customer item lists/options.

## Out Of Scope

This phase does not cache operational documents:

- `tabPurchase Receipt`
- `tabPurchase Receipt Item`
- `tabDelivery Note`
- `tabDelivery Note Item`
- `tabStock Entry`
- `tabStock Entry Detail`
- `tabComment`

This phase also does not cache or replace admin credentials:

- `tabUser`
- `__Auth`

ERPNext writes and submit flows stay on the existing ERPNext client/direct DB paths.

## Architecture

The server gets a new catalog cache module with three responsibilities:

1. SQLite schema and local query functions.
2. Sync from ERPNext MariaDB through the existing direct DB connection.
3. A read adapter implementing existing read ports from SQLite.

The initial wiring is feature-flagged:

- `ERP_CATALOG_CACHE_ENABLED=1` enables catalog cache reads.
- `ERP_CATALOG_CACHE_FALLBACK_DIRECT_DB=1` lets the server fall back to the existing `DirectDbReader` when cache is unavailable.
- `ERP_CATALOG_CACHE_PATH` controls the SQLite file path and defaults to `data/catalog_cache.sqlite`.

The existing `DirectDbReader` remains available. The new adapter should be introduced without changing HTTP contracts or mobile app behavior.

## Data Flow

On server startup, if catalog cache is enabled:

1. Open or create the SQLite catalog cache.
2. Run schema migrations.
3. Start an initial sync from MariaDB.
4. Wire catalog read ports to SQLite-backed adapter after the cache is usable.

After startup, sync runs periodically:

1. Read rows modified since the last sync watermark.
2. Upsert changed rows into SQLite.
3. Periodically run a full reconcile for deletes and missed changes.

For this first implementation, a full sync is acceptable as the first working milestone because the current catalog size is small. Delta sync and reconcile can be added behind the same cache interface.

## SQLite Schema

The cache tables store only fields the RS server already reads:

- `catalog_items`: `name`, `item_name`, `stock_uom`, `item_group`, `modified`, `disabled`, `is_stock_item`
- `catalog_item_groups`: `name`, `item_group_name`, `parent_item_group`, `is_group`, `lft`, `modified`
- `catalog_suppliers`: `name`, `supplier_name`, `mobile_no`, `disabled`, `modified`
- `catalog_customers`: `name`, `customer_name`, `mobile_no`, `disabled`, `modified`
- `catalog_item_suppliers`: `parent`, `supplier`, `modified`
- `catalog_item_customers`: `parent`, `customer_name`, `modified`
- `catalog_sync_state`: `scope`, `last_full_sync_at`, `last_delta_sync_at`, `last_modified`

Indexes must cover:

- item search by `name` and `item_name`
- item filtering by `item_group`
- group ordering by `lft`
- supplier mapping by `supplier`
- customer mapping by `customer_name`

## Read Coverage

The SQLite adapter should implement these existing contracts first:

- `AdminReadPort::items_page`
- `AdminReadPort::items_page_by_group`
- `AdminReadPort::items_by_codes`
- `AdminReadPort::item_groups`
- `AdminReadPort::item_group_tree`
- `AdminReadPort::suppliers_page`
- `AdminReadPort::supplier_by_ref`
- `AdminReadPort::customers_page`
- `AdminReadPort::customer_by_ref`
- `AdminReadPort::assigned_supplier_items`
- `AdminReadPort::customer_items`
- `WerkaHomeLookup::werka_suppliers`
- `WerkaHomeLookup::werka_customers`
- `WerkaHomeLookup::werka_supplier_items`
- `WerkaHomeLookup::werka_customer_items`
- `WerkaHomeLookup::werka_customer_item_options`
- `ProfileLookup::get_supplier_profile`
- `ProfileLookup::get_customer_profile`

Operational `WerkaHomeLookup` methods such as summary, pending, history, archive, stock-entry barcode lookup, notification detail, and customer issue source stay on direct DB for now.

## Failure Handling

If SQLite open, migration, or sync fails:

- log the exact error;
- keep serving through `DirectDbReader` when fallback is enabled;
- fail startup only when cache is enabled and fallback is disabled.

If a cache query fails at runtime:

- return fallback direct DB result when fallback is enabled;
- return the existing port error when fallback is disabled.

## Testing

Tests must prove:

- schema migration creates all tables and indexes;
- sync converts MariaDB-style rows into SQLite rows;
- SQLite item search matches direct DB behavior for query, limit, and offset;
- item group tree preserves parent/child fields and `lft` order;
- supplier/customer item mapping returns the same model shapes as existing readers;
- fallback is used when cache is unavailable.

Real ERPNext verification should compare a small set of live reads:

- item list first page;
- item search for an existing product;
- item groups;
- supplier item list;
- customer item options.

## Rollout

1. Ship disabled by default.
2. Enable on staging/local ERPNext with fallback enabled.
3. Compare direct DB output and SQLite output.
4. Enable on production RS with fallback enabled.
5. After stable operation, keep fallback available but leave cache as the primary catalog read path.
