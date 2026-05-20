use sqlx::query_as;

use crate::erpdb::catalog_cache::store::{
    CachedCustomer, CachedItem, CachedItemCustomer, CachedItemGroup, CachedItemSupplier,
    CachedSupplier, CatalogCacheError, CatalogCacheStore, CatalogDeltaSnapshot, CatalogKeySnapshot,
    CatalogMissingChangedKeys, CatalogSnapshot, CatalogStatsSnapshot, CatalogTableStats,
};
use crate::erpdb::reader::DirectDbReader;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogSyncReport {
    pub items: usize,
    pub item_groups: usize,
    pub suppliers: usize,
    pub customers: usize,
    pub item_suppliers: usize,
    pub item_customers: usize,
}

pub async fn sync_catalog_once(
    direct: &DirectDbReader,
    store: &CatalogCacheStore,
) -> Result<CatalogSyncReport, CatalogCacheError> {
    let items = query_as::<_, ItemRow>(ITEMS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_groups = query_as::<_, ItemGroupRow>(ITEM_GROUPS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let suppliers = query_as::<_, SupplierRow>(SUPPLIERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let customers = query_as::<_, CustomerRow>(CUSTOMERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_suppliers = query_as::<_, ItemSupplierRow>(ITEM_SUPPLIERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_customers = query_as::<_, ItemCustomerRow>(ITEM_CUSTOMERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;

    store.replace_catalog(CatalogSnapshot {
        items: items.iter().map(ItemRow::to_cached).collect(),
        item_groups: item_groups.iter().map(ItemGroupRow::to_cached).collect(),
        suppliers: suppliers.iter().map(SupplierRow::to_cached).collect(),
        customers: customers.iter().map(CustomerRow::to_cached).collect(),
        item_suppliers: item_suppliers
            .iter()
            .map(ItemSupplierRow::to_cached)
            .collect(),
        item_customers: item_customers
            .iter()
            .map(ItemCustomerRow::to_cached)
            .collect(),
    })?;

    Ok(CatalogSyncReport {
        items: items.len(),
        item_groups: item_groups.len(),
        suppliers: suppliers.len(),
        customers: customers.len(),
        item_suppliers: item_suppliers.len(),
        item_customers: item_customers.len(),
    })
}

pub async fn sync_catalog_delta_once(
    direct: &DirectDbReader,
    store: &CatalogCacheStore,
) -> Result<CatalogSyncReport, CatalogCacheError> {
    let local_stats = store.stats()?;
    let remote_stats = fetch_catalog_stats(direct).await?;
    let items = query_as::<_, ItemRow>(CHANGED_ITEMS_SQL)
        .bind(&local_stats.items.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_groups = query_as::<_, ItemGroupRow>(CHANGED_ITEM_GROUPS_SQL)
        .bind(&local_stats.item_groups.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let suppliers = query_as::<_, SupplierRow>(CHANGED_SUPPLIERS_SQL)
        .bind(&local_stats.suppliers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let customers = query_as::<_, CustomerRow>(CHANGED_CUSTOMERS_SQL)
        .bind(&local_stats.customers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_suppliers = query_as::<_, ItemSupplierRow>(CHANGED_ITEM_SUPPLIERS_SQL)
        .bind(&local_stats.item_suppliers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_customers = query_as::<_, ItemCustomerRow>(CHANGED_ITEM_CUSTOMERS_SQL)
        .bind(&local_stats.item_customers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let changed = CatalogSnapshot {
        items: items.iter().map(ItemRow::to_cached).collect(),
        item_groups: item_groups.iter().map(ItemGroupRow::to_cached).collect(),
        suppliers: suppliers.iter().map(SupplierRow::to_cached).collect(),
        customers: customers.iter().map(CustomerRow::to_cached).collect(),
        item_suppliers: item_suppliers
            .iter()
            .map(ItemSupplierRow::to_cached)
            .collect(),
        item_customers: item_customers
            .iter()
            .map(ItemCustomerRow::to_cached)
            .collect(),
    };
    let missing_changed_keys = store.missing_changed_keys(&changed)?;
    let keys =
        fetch_catalog_keys(direct, &local_stats, &remote_stats, missing_changed_keys).await?;

    store.apply_delta(CatalogDeltaSnapshot { changed, keys })?;

    Ok(CatalogSyncReport {
        items: items.len(),
        item_groups: item_groups.len(),
        suppliers: suppliers.len(),
        customers: customers.len(),
        item_suppliers: item_suppliers.len(),
        item_customers: item_customers.len(),
    })
}

async fn fetch_catalog_stats(
    direct: &DirectDbReader,
) -> Result<CatalogStatsSnapshot, CatalogCacheError> {
    Ok(CatalogStatsSnapshot {
        items: fetch_table_stats(direct, ITEM_STATS_SQL).await?,
        item_groups: fetch_table_stats(direct, ITEM_GROUP_STATS_SQL).await?,
        suppliers: fetch_table_stats(direct, SUPPLIER_STATS_SQL).await?,
        customers: fetch_table_stats(direct, CUSTOMER_STATS_SQL).await?,
        item_suppliers: fetch_table_stats(direct, ITEM_SUPPLIER_STATS_SQL).await?,
        item_customers: fetch_table_stats(direct, ITEM_CUSTOMER_STATS_SQL).await?,
    })
}

async fn fetch_table_stats(
    direct: &DirectDbReader,
    sql: &str,
) -> Result<CatalogTableStats, CatalogCacheError> {
    let row = query_as::<_, TableStatsRow>(sql)
        .fetch_one(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    Ok(CatalogTableStats {
        count: row.row_count,
        max_modified: row.max_modified.trim().to_string(),
    })
}

async fn fetch_catalog_keys(
    direct: &DirectDbReader,
    local_stats: &CatalogStatsSnapshot,
    remote_stats: &CatalogStatsSnapshot,
    missing_changed_keys: CatalogMissingChangedKeys,
) -> Result<CatalogKeySnapshot, CatalogCacheError> {
    Ok(CatalogKeySnapshot {
        items: fetch_single_keys_if_count_changed(
            direct,
            ITEM_KEYS_SQL,
            local_stats.items.count,
            remote_stats.items.count,
            missing_changed_keys.items,
        )
        .await?,
        item_groups: fetch_single_keys_if_count_changed(
            direct,
            ITEM_GROUP_KEYS_SQL,
            local_stats.item_groups.count,
            remote_stats.item_groups.count,
            missing_changed_keys.item_groups,
        )
        .await?,
        suppliers: fetch_single_keys_if_count_changed(
            direct,
            SUPPLIER_KEYS_SQL,
            local_stats.suppliers.count,
            remote_stats.suppliers.count,
            missing_changed_keys.suppliers,
        )
        .await?,
        customers: fetch_single_keys_if_count_changed(
            direct,
            CUSTOMER_KEYS_SQL,
            local_stats.customers.count,
            remote_stats.customers.count,
            missing_changed_keys.customers,
        )
        .await?,
        item_suppliers: fetch_composite_keys_if_count_changed(
            direct,
            ITEM_SUPPLIER_KEYS_SQL,
            local_stats.item_suppliers.count,
            remote_stats.item_suppliers.count,
            missing_changed_keys.item_suppliers,
        )
        .await?,
        item_customers: fetch_composite_keys_if_count_changed(
            direct,
            ITEM_CUSTOMER_KEYS_SQL,
            local_stats.item_customers.count,
            remote_stats.item_customers.count,
            missing_changed_keys.item_customers,
        )
        .await?,
    })
}

async fn fetch_single_keys_if_count_changed(
    direct: &DirectDbReader,
    sql: &str,
    local_count: i64,
    remote_count: i64,
    force: bool,
) -> Result<Option<Vec<String>>, CatalogCacheError> {
    if local_count == remote_count && !force {
        return Ok(None);
    }
    Ok(Some(
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(&direct.pool)
            .await
            .map_err(map_sqlx)?
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect(),
    ))
}

async fn fetch_composite_keys_if_count_changed(
    direct: &DirectDbReader,
    sql: &str,
    local_count: i64,
    remote_count: i64,
    force: bool,
) -> Result<Option<Vec<(String, String)>>, CatalogCacheError> {
    if local_count == remote_count && !force {
        return Ok(None);
    }
    Ok(Some(
        query_as::<_, CompositeKeyRow>(sql)
            .fetch_all(&direct.pool)
            .await
            .map_err(map_sqlx)?
            .into_iter()
            .map(|row| {
                (
                    row.left_key.trim().to_string(),
                    row.right_key.trim().to_string(),
                )
            })
            .collect(),
    ))
}

#[derive(Debug, sqlx::FromRow)]
struct ItemRow {
    name: String,
    item_name: String,
    stock_uom: String,
    item_group: String,
    modified: String,
    disabled: i32,
    is_stock_item: i32,
}

impl ItemRow {
    fn to_cached(&self) -> CachedItem {
        CachedItem {
            name: self.name.trim().to_string(),
            item_name: self.item_name.trim().to_string(),
            stock_uom: self.stock_uom.trim().to_string(),
            item_group: self.item_group.trim().to_string(),
            modified: self.modified.trim().to_string(),
            disabled: self.disabled != 0,
            is_stock_item: self.is_stock_item != 0,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ItemGroupRow {
    name: String,
    item_group_name: String,
    parent_item_group: String,
    is_group: i32,
    lft: i64,
    modified: String,
}

impl ItemGroupRow {
    fn to_cached(&self) -> CachedItemGroup {
        CachedItemGroup {
            name: self.name.trim().to_string(),
            item_group_name: self.item_group_name.trim().to_string(),
            parent_item_group: self.parent_item_group.trim().to_string(),
            is_group: self.is_group != 0,
            lft: self.lft,
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SupplierRow {
    name: String,
    supplier_name: String,
    mobile_no: String,
    supplier_details: String,
    image: String,
    disabled: i32,
    modified: String,
}

impl SupplierRow {
    fn to_cached(&self) -> CachedSupplier {
        CachedSupplier {
            name: self.name.trim().to_string(),
            supplier_name: self.supplier_name.trim().to_string(),
            mobile_no: self.mobile_no.trim().to_string(),
            supplier_details: self.supplier_details.trim().to_string(),
            image: self.image.trim().to_string(),
            disabled: self.disabled != 0,
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerRow {
    name: String,
    customer_name: String,
    mobile_no: String,
    customer_details: String,
    disabled: i32,
    modified: String,
}

impl CustomerRow {
    fn to_cached(&self) -> CachedCustomer {
        CachedCustomer {
            name: self.name.trim().to_string(),
            customer_name: self.customer_name.trim().to_string(),
            mobile_no: self.mobile_no.trim().to_string(),
            customer_details: self.customer_details.trim().to_string(),
            disabled: self.disabled != 0,
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ItemSupplierRow {
    parent: String,
    supplier: String,
    modified: String,
}

impl ItemSupplierRow {
    fn to_cached(&self) -> CachedItemSupplier {
        CachedItemSupplier {
            parent: self.parent.trim().to_string(),
            supplier: self.supplier.trim().to_string(),
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ItemCustomerRow {
    parent: String,
    customer_name: String,
    modified: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CompositeKeyRow {
    left_key: String,
    right_key: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TableStatsRow {
    row_count: i64,
    max_modified: String,
}

impl ItemCustomerRow {
    fn to_cached(&self) -> CachedItemCustomer {
        CachedItemCustomer {
            parent: self.parent.trim().to_string(),
            customer_name: self.customer_name.trim().to_string(),
            modified: self.modified.trim().to_string(),
        }
    }
}

fn map_sqlx(error: sqlx::Error) -> CatalogCacheError {
    CatalogCacheError::Sync(error.to_string())
}

const ITEMS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_name, '') AS item_name,
        COALESCE(stock_uom, '') AS stock_uom,
        COALESCE(item_group, '') AS item_group,
        COALESCE(CAST(modified AS CHAR), '') AS modified,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(is_stock_item, 0) AS is_stock_item
    FROM tabItem
"#;

const CHANGED_ITEMS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_name, '') AS item_name,
        COALESCE(stock_uom, '') AS stock_uom,
        COALESCE(item_group, '') AS item_group,
        COALESCE(CAST(modified AS CHAR), '') AS modified,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(is_stock_item, 0) AS is_stock_item
    FROM tabItem
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

const ITEM_GROUPS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_group_name, '') AS item_group_name,
        COALESCE(parent_item_group, '') AS parent_item_group,
        COALESCE(is_group, 0) AS is_group,
        COALESCE(lft, 0) AS lft,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Group`
"#;

const CHANGED_ITEM_GROUPS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_group_name, '') AS item_group_name,
        COALESCE(parent_item_group, '') AS parent_item_group,
        COALESCE(is_group, 0) AS is_group,
        COALESCE(lft, 0) AS lft,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Group`
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

const SUPPLIERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(supplier_name, '') AS supplier_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(supplier_details, '') AS supplier_details,
        COALESCE(image, '') AS image,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabSupplier
"#;

const CHANGED_SUPPLIERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(supplier_name, '') AS supplier_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(supplier_details, '') AS supplier_details,
        COALESCE(image, '') AS image,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabSupplier
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

const CUSTOMERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(customer_name, '') AS customer_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(customer_details, '') AS customer_details,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabCustomer
"#;

const CHANGED_CUSTOMERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(customer_name, '') AS customer_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(customer_details, '') AS customer_details,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabCustomer
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

const ITEM_SUPPLIERS_SQL: &str = r#"
    SELECT
        parent,
        supplier,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Supplier`
"#;

const CHANGED_ITEM_SUPPLIERS_SQL: &str = r#"
    SELECT
        parent,
        supplier,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Supplier`
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

const ITEM_CUSTOMERS_SQL: &str = r#"
    SELECT
        parent,
        customer_name,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Customer Detail`
"#;

const CHANGED_ITEM_CUSTOMERS_SQL: &str = r#"
    SELECT
        parent,
        customer_name,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Customer Detail`
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

const ITEM_KEYS_SQL: &str = "SELECT name FROM tabItem";
const ITEM_GROUP_KEYS_SQL: &str = "SELECT name FROM `tabItem Group`";
const SUPPLIER_KEYS_SQL: &str = "SELECT name FROM tabSupplier";
const CUSTOMER_KEYS_SQL: &str = "SELECT name FROM tabCustomer";
const ITEM_SUPPLIER_KEYS_SQL: &str =
    "SELECT parent AS left_key, supplier AS right_key FROM `tabItem Supplier`";
const ITEM_CUSTOMER_KEYS_SQL: &str =
    "SELECT parent AS left_key, customer_name AS right_key FROM `tabItem Customer Detail`";

const ITEM_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM tabItem";
const ITEM_GROUP_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM `tabItem Group`";
const SUPPLIER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM tabSupplier";
const CUSTOMER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM tabCustomer";
const ITEM_SUPPLIER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM `tabItem Supplier`";
const ITEM_CUSTOMER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM `tabItem Customer Detail`";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_mappers_trim_and_convert_flags() {
        let item = ItemRow {
            name: " ITEM-001 ".to_string(),
            item_name: " Alma ".to_string(),
            stock_uom: " Kg ".to_string(),
            item_group: " Finished ".to_string(),
            modified: " 2026-05-20 ".to_string(),
            disabled: 0,
            is_stock_item: 1,
        }
        .to_cached();

        assert_eq!(item.name, "ITEM-001");
        assert_eq!(item.item_name, "Alma");
        assert!(!item.disabled);
        assert!(item.is_stock_item);

        let supplier = SupplierRow {
            name: " SUP-001 ".to_string(),
            supplier_name: " Best ".to_string(),
            mobile_no: " +99890 ".to_string(),
            supplier_details: " Phone: +99891 ".to_string(),
            image: " /files/a.png ".to_string(),
            disabled: 1,
            modified: String::new(),
        }
        .to_cached();

        assert_eq!(supplier.name, "SUP-001");
        assert_eq!(supplier.image, "/files/a.png");
        assert!(supplier.disabled);
    }

    #[tokio::test]
    #[ignore = "requires ERP_TEST_SITE_CONFIG and local MariaDB"]
    async fn real_erp_catalog_cache_matches_direct_db_reads() {
        use std::sync::Arc;

        use crate::config::AppConfig;
        use crate::core::admin::ports::AdminReadPort;
        use crate::core::profile::ports::ProfileLookup;
        use crate::core::werka::ports::WerkaHomeLookup;
        use crate::erpdb::catalog_cache::reader::CatalogCacheReader;

        let config = AppConfig::from_env().expect("app config");
        let direct_config = config
            .direct_db_config()
            .expect("direct db config")
            .expect("ERP_DIRECT_READ_ENABLED=1");
        let direct = Arc::new(DirectDbReader::new(direct_config.clone()));
        let store = Arc::new(CatalogCacheStore::in_memory().expect("cache"));

        let report = sync_catalog_once(&direct, &store).await.expect("sync");
        assert!(report.items > 0, "expected ERP items");
        assert!(report.item_groups > 0, "expected ERP item groups");

        let cached = CatalogCacheReader::new(store, direct_config.default_warehouse.clone())
            .with_fallback(direct.clone());

        assert_eq!(
            AdminReadPort::items_page(&cached, "", 80, 0).await.unwrap(),
            AdminReadPort::items_page(direct.as_ref(), "", 80, 0)
                .await
                .unwrap()
        );
        assert_eq!(
            AdminReadPort::item_group_tree(&cached).await.unwrap(),
            AdminReadPort::item_group_tree(direct.as_ref())
                .await
                .unwrap()
        );
        assert_eq!(
            WerkaHomeLookup::werka_suppliers(&cached, "", 50, 0)
                .await
                .unwrap(),
            WerkaHomeLookup::werka_suppliers(direct.as_ref(), "", 50, 0)
                .await
                .unwrap()
        );
        assert_eq!(
            WerkaHomeLookup::werka_customers(&cached, "", 50, 0)
                .await
                .unwrap(),
            WerkaHomeLookup::werka_customers(direct.as_ref(), "", 50, 0)
                .await
                .unwrap()
        );

        if let Some(supplier) = WerkaHomeLookup::werka_suppliers(direct.as_ref(), "", 1, 0)
            .await
            .unwrap()
            .first()
        {
            assert_eq!(
                WerkaHomeLookup::werka_supplier_items(&cached, &supplier.ref_, "", 80, 0)
                    .await
                    .unwrap(),
                WerkaHomeLookup::werka_supplier_items(direct.as_ref(), &supplier.ref_, "", 80, 0)
                    .await
                    .unwrap()
            );
            assert_eq!(
                ProfileLookup::get_supplier_profile(&cached, &supplier.ref_)
                    .await
                    .unwrap(),
                ProfileLookup::get_supplier_profile(direct.as_ref(), &supplier.ref_)
                    .await
                    .unwrap()
            );
        }

        if let Some(customer) = WerkaHomeLookup::werka_customers(direct.as_ref(), "", 1, 0)
            .await
            .unwrap()
            .first()
        {
            assert_eq!(
                WerkaHomeLookup::werka_customer_items(&cached, &customer.ref_, "", 80, 0)
                    .await
                    .unwrap(),
                WerkaHomeLookup::werka_customer_items(direct.as_ref(), &customer.ref_, "", 80, 0)
                    .await
                    .unwrap()
            );
            assert_eq!(
                ProfileLookup::get_customer_profile(&cached, &customer.ref_)
                    .await
                    .unwrap(),
                ProfileLookup::get_customer_profile(direct.as_ref(), &customer.ref_)
                    .await
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    #[ignore = "mutates local ERPNext test DB; requires ERP_DIRECT_* env"]
    async fn real_erp_catalog_cache_reflects_add_update_and_delete() {
        use std::time::Instant;

        use crate::config::AppConfig;

        let config = AppConfig::from_env().expect("app config");
        let direct_config = config
            .direct_db_config()
            .expect("direct db config")
            .expect("ERP_DIRECT_READ_ENABLED=1");
        let direct = DirectDbReader::new(direct_config.clone());
        let store = CatalogCacheStore::in_memory().expect("cache");
        let code = format!(
            "__rs_cache_test_{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        );

        cleanup_test_item(&direct, &code).await;
        insert_test_item(&direct, &code, "RS Cache Test Item")
            .await
            .expect("insert test item");
        let add_sync_started = Instant::now();
        sync_catalog_once(&direct, &store)
            .await
            .expect("sync after add");
        let add_sync_elapsed = add_sync_started.elapsed();
        assert_eq!(
            store
                .items_by_codes(
                    std::slice::from_ref(&code),
                    &direct_config.default_warehouse
                )
                .expect("cache item")[0]
                .name,
            "RS Cache Test Item"
        );

        update_test_item(&direct, &code, "RS Cache Test Item Updated")
            .await
            .expect("update test item");
        let update_sync_started = Instant::now();
        sync_catalog_once(&direct, &store)
            .await
            .expect("sync after update");
        let update_sync_elapsed = update_sync_started.elapsed();
        assert_eq!(
            store
                .items_by_codes(
                    std::slice::from_ref(&code),
                    &direct_config.default_warehouse
                )
                .expect("cache item")[0]
                .name,
            "RS Cache Test Item Updated"
        );

        cleanup_test_item(&direct, &code).await;
        let delete_sync_started = Instant::now();
        sync_catalog_once(&direct, &store)
            .await
            .expect("sync after delete");
        let delete_sync_elapsed = delete_sync_started.elapsed();
        assert!(
            store
                .items_by_codes(
                    std::slice::from_ref(&code),
                    &direct_config.default_warehouse
                )
                .expect("cache item")
                .is_empty()
        );

        println!(
            "real ERP catalog sync elapsed: add={:.3}ms update={:.3}ms delete={:.3}ms",
            add_sync_elapsed.as_secs_f64() * 1000.0,
            update_sync_elapsed.as_secs_f64() * 1000.0,
            delete_sync_elapsed.as_secs_f64() * 1000.0,
        );
    }

    #[tokio::test]
    #[ignore = "mutates local ERPNext test DB; requires ERP_DIRECT_* env"]
    async fn real_erp_catalog_delta_sync_reflects_add_update_and_delete() {
        use std::time::Instant;

        use crate::config::AppConfig;

        let config = AppConfig::from_env().expect("app config");
        let direct_config = config
            .direct_db_config()
            .expect("direct db config")
            .expect("ERP_DIRECT_READ_ENABLED=1");
        let direct = DirectDbReader::new(direct_config.clone());
        let store = CatalogCacheStore::in_memory().expect("cache");
        let code = format!(
            "__rs_cache_delta_test_{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        );

        cleanup_test_item(&direct, &code).await;
        sync_catalog_once(&direct, &store)
            .await
            .expect("initial sync");

        insert_test_item(&direct, &code, "RS Cache Delta Test Item")
            .await
            .expect("insert test item");
        let add_sync_started = Instant::now();
        sync_catalog_delta_once(&direct, &store)
            .await
            .expect("delta sync after add");
        let add_sync_elapsed = add_sync_started.elapsed();
        assert_eq!(
            store
                .items_by_codes(
                    std::slice::from_ref(&code),
                    &direct_config.default_warehouse
                )
                .expect("cache item")[0]
                .name,
            "RS Cache Delta Test Item"
        );

        update_test_item(&direct, &code, "RS Cache Delta Test Item Updated")
            .await
            .expect("update test item");
        let update_sync_started = Instant::now();
        sync_catalog_delta_once(&direct, &store)
            .await
            .expect("delta sync after update");
        let update_sync_elapsed = update_sync_started.elapsed();
        assert_eq!(
            store
                .items_by_codes(
                    std::slice::from_ref(&code),
                    &direct_config.default_warehouse
                )
                .expect("cache item")[0]
                .name,
            "RS Cache Delta Test Item Updated"
        );

        cleanup_test_item(&direct, &code).await;
        let delete_sync_started = Instant::now();
        sync_catalog_delta_once(&direct, &store)
            .await
            .expect("delta sync after delete");
        let delete_sync_elapsed = delete_sync_started.elapsed();
        assert!(
            store
                .items_by_codes(
                    std::slice::from_ref(&code),
                    &direct_config.default_warehouse
                )
                .expect("cache item")
                .is_empty()
        );

        let swap_old_code = format!(
            "__rs_cache_delta_old_{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        );
        let swap_new_code = format!("{swap_old_code}_new");
        cleanup_test_item(&direct, &swap_old_code).await;
        cleanup_test_item(&direct, &swap_new_code).await;
        insert_test_item(&direct, &swap_old_code, "RS Cache Delta Old Item")
            .await
            .expect("insert old swap item");
        sync_catalog_delta_once(&direct, &store)
            .await
            .expect("delta sync old swap item");
        assert_eq!(
            store
                .items_by_codes(
                    std::slice::from_ref(&swap_old_code),
                    &direct_config.default_warehouse
                )
                .expect("old cache item")
                .len(),
            1
        );

        insert_test_item(&direct, &swap_new_code, "RS Cache Delta New Item")
            .await
            .expect("insert new swap item");
        cleanup_test_item(&direct, &swap_old_code).await;
        let same_count_swap_sync_started = Instant::now();
        sync_catalog_delta_once(&direct, &store)
            .await
            .expect("delta sync same-count swap");
        let same_count_swap_sync_elapsed = same_count_swap_sync_started.elapsed();
        assert!(
            store
                .items_by_codes(
                    std::slice::from_ref(&swap_old_code),
                    &direct_config.default_warehouse
                )
                .expect("old cache item")
                .is_empty()
        );
        assert_eq!(
            store
                .items_by_codes(
                    std::slice::from_ref(&swap_new_code),
                    &direct_config.default_warehouse
                )
                .expect("new cache item")[0]
                .name,
            "RS Cache Delta New Item"
        );
        cleanup_test_item(&direct, &swap_new_code).await;

        println!(
            "real ERP catalog delta sync elapsed: add={:.3}ms update={:.3}ms delete={:.3}ms same_count_swap={:.3}ms",
            add_sync_elapsed.as_secs_f64() * 1000.0,
            update_sync_elapsed.as_secs_f64() * 1000.0,
            delete_sync_elapsed.as_secs_f64() * 1000.0,
            same_count_swap_sync_elapsed.as_secs_f64() * 1000.0,
        );
    }

    #[tokio::test]
    #[ignore = "mutates local ERPNext test DB; requires ERP_DIRECT_* env"]
    async fn real_erp_catalog_delta_sync_reflects_all_cached_read_scopes() {
        use std::sync::Arc;
        use std::time::Duration;

        use crate::config::AppConfig;
        use crate::core::admin::ports::AdminReadPort;
        use crate::core::profile::ports::ProfileLookup;
        use crate::erpdb::catalog_cache::reader::CatalogCacheReader;

        let config = AppConfig::from_env().expect("app config");
        let direct_config = config
            .direct_db_config()
            .expect("direct db config")
            .expect("ERP_DIRECT_READ_ENABLED=1");
        let direct = DirectDbReader::new(direct_config.clone());
        let prefix = format!(
            "__rs_cache_all_{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        );
        cleanup_catalog_prefix(&direct, &prefix).await;

        let store = Arc::new(CatalogCacheStore::in_memory().expect("cache"));
        sync_catalog_once(&direct, &store)
            .await
            .expect("initial sync");
        let cached =
            CatalogCacheReader::new(store.clone(), direct_config.default_warehouse.clone())
                .with_fallback(Arc::new(direct.clone()));
        let mut timings: Vec<(&'static str, Duration)> = Vec::new();

        let group = format!("{prefix}_group");
        let group_name = format!("{prefix} Group");
        let group_name_updated = format!("{prefix} Group Updated");
        insert_test_item_group(&direct, &group, &group_name)
            .await
            .expect("insert item group");
        measured_delta_sync("item_group:add", &direct, &store, &mut timings).await;
        assert_contains(
            &AdminReadPort::item_groups(&cached, &group, 20)
                .await
                .expect("item groups"),
            &group,
        );
        assert_item_group_tree_name(&cached, &group, &group_name).await;

        update_test_item_group(&direct, &group, &group_name_updated)
            .await
            .expect("update item group");
        measured_delta_sync("item_group:update", &direct, &store, &mut timings).await;
        assert_item_group_tree_name(&cached, &group, &group_name_updated).await;

        cleanup_test_item_group(&direct, &group).await;
        measured_delta_sync("item_group:delete", &direct, &store, &mut timings).await;
        assert_not_contains(
            &AdminReadPort::item_groups(&cached, &group, 20)
                .await
                .expect("item groups after delete"),
            &group,
        );

        let group_swap_old = format!("{prefix}_group_swap_old");
        let group_swap_new = format!("{prefix}_group_swap_new");
        insert_test_item_group(
            &direct,
            &group_swap_old,
            &format!("{prefix} Group Swap Old"),
        )
        .await
        .expect("insert old swap group");
        measured_delta_sync("item_group:swap_old_add", &direct, &store, &mut timings).await;
        insert_test_item_group(
            &direct,
            &group_swap_new,
            &format!("{prefix} Group Swap New"),
        )
        .await
        .expect("insert new swap group");
        cleanup_test_item_group(&direct, &group_swap_old).await;
        measured_delta_sync("item_group:same_count_swap", &direct, &store, &mut timings).await;
        assert_not_contains(
            &AdminReadPort::item_groups(&cached, &group_swap_old, 20)
                .await
                .expect("old swapped item group"),
            &group_swap_old,
        );
        assert_contains(
            &AdminReadPort::item_groups(&cached, &group_swap_new, 20)
                .await
                .expect("new swapped item group"),
            &group_swap_new,
        );

        let supplier = format!("{prefix}_supplier");
        insert_test_supplier(&direct, &supplier, "RS Cache All Supplier", "+998900001111")
            .await
            .expect("insert supplier");
        measured_delta_sync("supplier:add", &direct, &store, &mut timings).await;
        assert_eq!(
            AdminReadPort::supplier_by_ref(&cached, &supplier)
                .await
                .expect("supplier")
                .name,
            "RS Cache All Supplier"
        );
        assert_eq!(
            ProfileLookup::get_supplier_profile(&cached, &supplier)
                .await
                .expect("supplier profile")
                .phone,
            "+998900001111"
        );

        update_test_supplier(
            &direct,
            &supplier,
            "RS Cache All Supplier Updated",
            "+998900002222",
        )
        .await
        .expect("update supplier");
        measured_delta_sync("supplier:update", &direct, &store, &mut timings).await;
        assert_eq!(
            AdminReadPort::supplier_by_ref(&cached, &supplier)
                .await
                .expect("updated supplier")
                .name,
            "RS Cache All Supplier Updated"
        );
        assert_eq!(
            ProfileLookup::get_supplier_profile(&cached, &supplier)
                .await
                .expect("updated supplier profile")
                .phone,
            "+998900002222"
        );

        cleanup_test_supplier(&direct, &supplier).await;
        measured_delta_sync("supplier:delete", &direct, &store, &mut timings).await;
        assert!(
            AdminReadPort::supplier_by_ref(&cached, &supplier)
                .await
                .is_err()
        );

        let supplier_swap_old = format!("{prefix}_supplier_swap_old");
        let supplier_swap_new = format!("{prefix}_supplier_swap_new");
        insert_test_supplier(
            &direct,
            &supplier_swap_old,
            "RS Cache Swap Supplier Old",
            "+998900003333",
        )
        .await
        .expect("insert old swap supplier");
        measured_delta_sync("supplier:swap_old_add", &direct, &store, &mut timings).await;
        insert_test_supplier(
            &direct,
            &supplier_swap_new,
            "RS Cache Swap Supplier New",
            "+998900004444",
        )
        .await
        .expect("insert new swap supplier");
        cleanup_test_supplier(&direct, &supplier_swap_old).await;
        measured_delta_sync("supplier:same_count_swap", &direct, &store, &mut timings).await;
        assert!(
            AdminReadPort::supplier_by_ref(&cached, &supplier_swap_old)
                .await
                .is_err()
        );
        assert_eq!(
            AdminReadPort::supplier_by_ref(&cached, &supplier_swap_new)
                .await
                .expect("new swapped supplier")
                .name,
            "RS Cache Swap Supplier New"
        );

        let customer = format!("{prefix}_customer");
        insert_test_customer(&direct, &customer, "RS Cache All Customer", "+998901112222")
            .await
            .expect("insert customer");
        measured_delta_sync("customer:add", &direct, &store, &mut timings).await;
        assert_eq!(
            AdminReadPort::customer_by_ref(&cached, &customer)
                .await
                .expect("customer")
                .name,
            "RS Cache All Customer"
        );
        assert_eq!(
            ProfileLookup::get_customer_profile(&cached, &customer)
                .await
                .expect("customer profile")
                .phone,
            "+998901112222"
        );

        update_test_customer(
            &direct,
            &customer,
            "RS Cache All Customer Updated",
            "+998903334444",
        )
        .await
        .expect("update customer");
        measured_delta_sync("customer:update", &direct, &store, &mut timings).await;
        assert_eq!(
            AdminReadPort::customer_by_ref(&cached, &customer)
                .await
                .expect("updated customer")
                .name,
            "RS Cache All Customer Updated"
        );
        assert_eq!(
            ProfileLookup::get_customer_profile(&cached, &customer)
                .await
                .expect("updated customer profile")
                .phone,
            "+998903334444"
        );

        cleanup_test_customer(&direct, &customer).await;
        measured_delta_sync("customer:delete", &direct, &store, &mut timings).await;
        assert!(
            AdminReadPort::customer_by_ref(&cached, &customer)
                .await
                .is_err()
        );

        let customer_swap_old = format!("{prefix}_customer_swap_old");
        let customer_swap_new = format!("{prefix}_customer_swap_new");
        insert_test_customer(
            &direct,
            &customer_swap_old,
            "RS Cache Swap Customer Old",
            "+998901115555",
        )
        .await
        .expect("insert old swap customer");
        measured_delta_sync("customer:swap_old_add", &direct, &store, &mut timings).await;
        insert_test_customer(
            &direct,
            &customer_swap_new,
            "RS Cache Swap Customer New",
            "+998901116666",
        )
        .await
        .expect("insert new swap customer");
        cleanup_test_customer(&direct, &customer_swap_old).await;
        measured_delta_sync("customer:same_count_swap", &direct, &store, &mut timings).await;
        assert!(
            AdminReadPort::customer_by_ref(&cached, &customer_swap_old)
                .await
                .is_err()
        );
        assert_eq!(
            AdminReadPort::customer_by_ref(&cached, &customer_swap_new)
                .await
                .expect("new swapped customer")
                .name,
            "RS Cache Swap Customer New"
        );

        let item = format!("{prefix}_item");
        insert_test_item(&direct, &item, "RS Cache All Item")
            .await
            .expect("insert item");
        measured_delta_sync("item:add", &direct, &store, &mut timings).await;
        assert_eq!(
            AdminReadPort::items_by_codes(&cached, std::slice::from_ref(&item))
                .await
                .expect("items by code")[0]
                .name,
            "RS Cache All Item"
        );
        assert_eq!(
            AdminReadPort::items_page(&cached, "RS Cache All Item", 20, 0)
                .await
                .expect("items page")[0]
                .code,
            item
        );
        assert_eq!(
            AdminReadPort::items_page_by_group(&cached, "Tayyor mahsulot", &item, 20, 0)
                .await
                .expect("items by group")[0]
                .code,
            item
        );

        update_test_item(&direct, &item, "RS Cache All Item Updated")
            .await
            .expect("update item");
        measured_delta_sync("item:update", &direct, &store, &mut timings).await;
        assert_eq!(
            AdminReadPort::items_by_codes(&cached, std::slice::from_ref(&item))
                .await
                .expect("updated items by code")[0]
                .name,
            "RS Cache All Item Updated"
        );

        let item_swap_old = format!("{prefix}_item_swap_old");
        let item_swap_new = format!("{prefix}_item_swap_new");
        insert_test_item(&direct, &item_swap_old, "RS Cache Swap Item Old")
            .await
            .expect("insert old swap item");
        measured_delta_sync("item:swap_old_add", &direct, &store, &mut timings).await;
        insert_test_item(&direct, &item_swap_new, "RS Cache Swap Item New")
            .await
            .expect("insert new swap item");
        cleanup_test_item(&direct, &item_swap_old).await;
        measured_delta_sync("item:same_count_swap", &direct, &store, &mut timings).await;
        assert!(
            AdminReadPort::items_by_codes(&cached, std::slice::from_ref(&item_swap_old))
                .await
                .expect("old swapped item")
                .is_empty()
        );
        assert_eq!(
            AdminReadPort::items_by_codes(&cached, std::slice::from_ref(&item_swap_new))
                .await
                .expect("new swapped item")[0]
                .name,
            "RS Cache Swap Item New"
        );

        let supplier_a = format!("{prefix}_map_supplier_a");
        let supplier_b = format!("{prefix}_map_supplier_b");
        insert_test_supplier(
            &direct,
            &supplier_a,
            "RS Cache Map Supplier A",
            "+998904441111",
        )
        .await
        .expect("insert mapping supplier a");
        insert_test_supplier(
            &direct,
            &supplier_b,
            "RS Cache Map Supplier B",
            "+998904442222",
        )
        .await
        .expect("insert mapping supplier b");
        measured_delta_sync(
            "supplier_mapping_anchors:add",
            &direct,
            &store,
            &mut timings,
        )
        .await;

        let item_supplier_link = format!("{prefix}_item_supplier");
        insert_test_item_supplier(&direct, &item_supplier_link, &item, &supplier_a)
            .await
            .expect("insert item supplier");
        measured_delta_sync("item_supplier:add", &direct, &store, &mut timings).await;
        assert_item_for_supplier(&cached, &supplier_a, &item).await;
        assert_werka_supplier_visible(&cached, &supplier_a).await;

        update_test_item_supplier(&direct, &item_supplier_link, &supplier_b)
            .await
            .expect("update item supplier");
        measured_delta_sync("item_supplier:update", &direct, &store, &mut timings).await;
        assert_no_item_for_supplier(&cached, &supplier_a, &item).await;
        assert_item_for_supplier(&cached, &supplier_b, &item).await;

        cleanup_test_item_supplier(&direct, &item_supplier_link).await;
        measured_delta_sync("item_supplier:delete", &direct, &store, &mut timings).await;
        assert_no_item_for_supplier(&cached, &supplier_b, &item).await;

        let customer_a = format!("{prefix}_map_customer_a");
        let customer_b = format!("{prefix}_map_customer_b");
        insert_test_customer(
            &direct,
            &customer_a,
            "RS Cache Map Customer A",
            "+998905551111",
        )
        .await
        .expect("insert mapping customer a");
        insert_test_customer(
            &direct,
            &customer_b,
            "RS Cache Map Customer B",
            "+998905552222",
        )
        .await
        .expect("insert mapping customer b");
        measured_delta_sync(
            "customer_mapping_anchors:add",
            &direct,
            &store,
            &mut timings,
        )
        .await;

        let item_customer_link = format!("{prefix}_item_customer");
        insert_test_item_customer(&direct, &item_customer_link, &item, &customer_a)
            .await
            .expect("insert item customer");
        measured_delta_sync("item_customer:add", &direct, &store, &mut timings).await;
        assert_item_for_customer(&cached, &customer_a, &item).await;
        assert_werka_customer_visible(&cached, &customer_a).await;
        assert_customer_item_option(&cached, &customer_a, &item).await;

        update_test_item_customer(&direct, &item_customer_link, &customer_b)
            .await
            .expect("update item customer");
        measured_delta_sync("item_customer:update", &direct, &store, &mut timings).await;
        assert_no_item_for_customer(&cached, &customer_a, &item).await;
        assert_item_for_customer(&cached, &customer_b, &item).await;

        cleanup_test_item_customer(&direct, &item_customer_link).await;
        measured_delta_sync("item_customer:delete", &direct, &store, &mut timings).await;
        assert_no_item_for_customer(&cached, &customer_b, &item).await;

        cleanup_test_item(&direct, &item).await;
        measured_delta_sync("item:delete", &direct, &store, &mut timings).await;
        assert!(
            AdminReadPort::items_by_codes(&cached, std::slice::from_ref(&item))
                .await
                .expect("deleted items by code")
                .is_empty()
        );

        cleanup_catalog_prefix(&direct, &prefix).await;
        println!("real ERP catalog all-scope delta timings:");
        for (label, elapsed) in timings {
            println!("{label}={:.3}ms", elapsed.as_secs_f64() * 1000.0);
        }
    }

    async fn measured_delta_sync(
        label: &'static str,
        direct: &DirectDbReader,
        store: &CatalogCacheStore,
        timings: &mut Vec<(&'static str, std::time::Duration)>,
    ) {
        let started = std::time::Instant::now();
        sync_catalog_delta_once(direct, store)
            .await
            .unwrap_or_else(|error| panic!("{label} failed: {error}"));
        timings.push((label, started.elapsed()));
    }

    fn assert_contains(values: &[String], needle: &str) {
        assert!(
            values.iter().any(|value| value == needle),
            "missing {needle} in {values:?}"
        );
    }

    fn assert_not_contains(values: &[String], needle: &str) {
        assert!(
            values.iter().all(|value| value != needle),
            "unexpected {needle} in {values:?}"
        );
    }

    async fn assert_item_group_tree_name(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        group: &str,
        expected_name: &str,
    ) {
        let groups = crate::core::admin::ports::AdminReadPort::item_group_tree(cached)
            .await
            .expect("item group tree");
        let found = groups
            .iter()
            .find(|entry| entry.name == group)
            .unwrap_or_else(|| panic!("missing item group {group} in {groups:?}"));
        assert_eq!(found.item_group_name, expected_name);
    }

    async fn assert_item_for_supplier(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        supplier: &str,
        item: &str,
    ) {
        let assigned =
            crate::core::admin::ports::AdminReadPort::assigned_supplier_items(cached, supplier, 20)
                .await
                .expect("assigned supplier items");
        assert!(
            assigned.iter().any(|entry| entry.code == item),
            "missing item {item} for supplier {supplier}: {assigned:?}"
        );
        let werka = crate::core::werka::ports::WerkaHomeLookup::werka_supplier_items(
            cached, supplier, "", 20, 0,
        )
        .await
        .expect("werka supplier items");
        assert!(
            werka.iter().any(|entry| entry.code == item),
            "missing werka item {item} for supplier {supplier}: {werka:?}"
        );
    }

    async fn assert_no_item_for_supplier(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        supplier: &str,
        item: &str,
    ) {
        let assigned =
            crate::core::admin::ports::AdminReadPort::assigned_supplier_items(cached, supplier, 20)
                .await
                .expect("assigned supplier items");
        assert!(
            assigned.iter().all(|entry| entry.code != item),
            "unexpected item {item} for supplier {supplier}: {assigned:?}"
        );
        let werka = crate::core::werka::ports::WerkaHomeLookup::werka_supplier_items(
            cached, supplier, "", 20, 0,
        )
        .await
        .expect("werka supplier items");
        assert!(
            werka.iter().all(|entry| entry.code != item),
            "unexpected werka item {item} for supplier {supplier}: {werka:?}"
        );
    }

    async fn assert_werka_supplier_visible(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        supplier: &str,
    ) {
        let suppliers =
            crate::core::werka::ports::WerkaHomeLookup::werka_suppliers(cached, supplier, 20, 0)
                .await
                .expect("werka suppliers");
        assert!(
            suppliers.iter().any(|entry| entry.ref_ == supplier),
            "missing werka supplier {supplier}: {suppliers:?}"
        );
    }

    async fn assert_item_for_customer(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        customer: &str,
        item: &str,
    ) {
        let assigned =
            crate::core::admin::ports::AdminReadPort::customer_items(cached, customer, "", 20)
                .await
                .expect("admin customer items");
        assert!(
            assigned.iter().any(|entry| entry.code == item),
            "missing admin item {item} for customer {customer}: {assigned:?}"
        );
        let werka = crate::core::werka::ports::WerkaHomeLookup::werka_customer_items(
            cached, customer, "", 20, 0,
        )
        .await
        .expect("werka customer items");
        assert!(
            werka.iter().any(|entry| entry.code == item),
            "missing werka item {item} for customer {customer}: {werka:?}"
        );
    }

    async fn assert_no_item_for_customer(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        customer: &str,
        item: &str,
    ) {
        let assigned =
            crate::core::admin::ports::AdminReadPort::customer_items(cached, customer, "", 20)
                .await
                .expect("admin customer items");
        assert!(
            assigned.iter().all(|entry| entry.code != item),
            "unexpected admin item {item} for customer {customer}: {assigned:?}"
        );
        let werka = crate::core::werka::ports::WerkaHomeLookup::werka_customer_items(
            cached, customer, "", 20, 0,
        )
        .await
        .expect("werka customer items");
        assert!(
            werka.iter().all(|entry| entry.code != item),
            "unexpected werka item {item} for customer {customer}: {werka:?}"
        );
    }

    async fn assert_werka_customer_visible(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        customer: &str,
    ) {
        let customers =
            crate::core::werka::ports::WerkaHomeLookup::werka_customers(cached, customer, 20, 0)
                .await
                .expect("werka customers");
        assert!(
            customers.iter().any(|entry| entry.ref_ == customer),
            "missing werka customer {customer}: {customers:?}"
        );
    }

    async fn assert_customer_item_option(
        cached: &crate::erpdb::catalog_cache::reader::CatalogCacheReader,
        customer: &str,
        item: &str,
    ) {
        let options = crate::core::werka::ports::WerkaHomeLookup::werka_customer_item_options(
            cached, item, 20, 0,
        )
        .await
        .expect("customer item options");
        assert!(
            options
                .iter()
                .any(|entry| entry.customer_ref == customer && entry.item_code == item),
            "missing customer item option customer={customer} item={item}: {options:?}"
        );
    }

    async fn insert_test_item(
        direct: &DirectDbReader,
        code: &str,
        item_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO tabItem
                (name, creation, modified, modified_by, owner, docstatus, idx,
                 item_code, item_name, item_group, stock_uom, disabled, is_stock_item)
            VALUES
                (?, NOW(6), NOW(6), 'Administrator', 'Administrator', 0, 0,
                 ?, ?, 'Tayyor mahsulot', 'Kg', 0, 1)
            "#,
        )
        .bind(code)
        .bind(code)
        .bind(item_name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn update_test_item(
        direct: &DirectDbReader,
        code: &str,
        item_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE tabItem
            SET item_name = ?, modified = NOW(6)
            WHERE name = ?
            "#,
        )
        .bind(item_name)
        .bind(code)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_test_item(direct: &DirectDbReader, code: &str) {
        let _ = sqlx::query("DELETE FROM tabItem WHERE name = ?")
            .bind(code)
            .execute(&direct.pool)
            .await;
    }

    async fn insert_test_item_group(
        direct: &DirectDbReader,
        name: &str,
        group_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO `tabItem Group`
                (name, creation, modified, modified_by, owner, docstatus, idx,
                 item_group_name, parent_item_group, is_group, lft, rgt)
            VALUES
                (?, NOW(6), NOW(6), 'Administrator', 'Administrator', 0, 0,
                 ?, 'All Item Groups', 0, 0, 0)
            "#,
        )
        .bind(name)
        .bind(group_name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn update_test_item_group(
        direct: &DirectDbReader,
        name: &str,
        group_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE `tabItem Group`
            SET item_group_name = ?, modified = NOW(6)
            WHERE name = ?
            "#,
        )
        .bind(group_name)
        .bind(name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_test_item_group(direct: &DirectDbReader, name: &str) {
        let _ = sqlx::query("DELETE FROM `tabItem Group` WHERE name = ?")
            .bind(name)
            .execute(&direct.pool)
            .await;
    }

    async fn insert_test_supplier(
        direct: &DirectDbReader,
        name: &str,
        supplier_name: &str,
        phone: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO tabSupplier
                (name, creation, modified, modified_by, owner, docstatus, idx,
                 supplier_name, mobile_no, supplier_details, image, disabled)
            VALUES
                (?, NOW(6), NOW(6), 'Administrator', 'Administrator', 0, 0,
                 ?, ?, '', '/files/rs-cache-test.png', 0)
            "#,
        )
        .bind(name)
        .bind(supplier_name)
        .bind(phone)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn update_test_supplier(
        direct: &DirectDbReader,
        name: &str,
        supplier_name: &str,
        phone: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE tabSupplier
            SET supplier_name = ?, mobile_no = ?, modified = NOW(6)
            WHERE name = ?
            "#,
        )
        .bind(supplier_name)
        .bind(phone)
        .bind(name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_test_supplier(direct: &DirectDbReader, name: &str) {
        let _ = sqlx::query("DELETE FROM tabSupplier WHERE name = ?")
            .bind(name)
            .execute(&direct.pool)
            .await;
    }

    async fn insert_test_customer(
        direct: &DirectDbReader,
        name: &str,
        customer_name: &str,
        phone: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO tabCustomer
                (name, creation, modified, modified_by, owner, docstatus, idx,
                 customer_name, mobile_no, customer_details, disabled)
            VALUES
                (?, NOW(6), NOW(6), 'Administrator', 'Administrator', 0, 0,
                 ?, ?, '', 0)
            "#,
        )
        .bind(name)
        .bind(customer_name)
        .bind(phone)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn update_test_customer(
        direct: &DirectDbReader,
        name: &str,
        customer_name: &str,
        phone: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE tabCustomer
            SET customer_name = ?, mobile_no = ?, modified = NOW(6)
            WHERE name = ?
            "#,
        )
        .bind(customer_name)
        .bind(phone)
        .bind(name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_test_customer(direct: &DirectDbReader, name: &str) {
        let _ = sqlx::query("DELETE FROM tabCustomer WHERE name = ?")
            .bind(name)
            .execute(&direct.pool)
            .await;
    }

    async fn insert_test_item_supplier(
        direct: &DirectDbReader,
        name: &str,
        item: &str,
        supplier: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO `tabItem Supplier`
                (name, creation, modified, modified_by, owner, docstatus, idx,
                 parent, supplier, parentfield, parenttype)
            VALUES
                (?, NOW(6), NOW(6), 'Administrator', 'Administrator', 0, 0,
                 ?, ?, 'supplier_items', 'Item')
            "#,
        )
        .bind(name)
        .bind(item)
        .bind(supplier)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn update_test_item_supplier(
        direct: &DirectDbReader,
        name: &str,
        supplier: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE `tabItem Supplier`
            SET supplier = ?, modified = NOW(6)
            WHERE name = ?
            "#,
        )
        .bind(supplier)
        .bind(name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_test_item_supplier(direct: &DirectDbReader, name: &str) {
        let _ = sqlx::query("DELETE FROM `tabItem Supplier` WHERE name = ?")
            .bind(name)
            .execute(&direct.pool)
            .await;
    }

    async fn insert_test_item_customer(
        direct: &DirectDbReader,
        name: &str,
        item: &str,
        customer: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO `tabItem Customer Detail`
                (name, creation, modified, modified_by, owner, docstatus, idx,
                 parent, customer_name, parentfield, parenttype)
            VALUES
                (?, NOW(6), NOW(6), 'Administrator', 'Administrator', 0, 0,
                 ?, ?, 'customer_items', 'Item')
            "#,
        )
        .bind(name)
        .bind(item)
        .bind(customer)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn update_test_item_customer(
        direct: &DirectDbReader,
        name: &str,
        customer: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE `tabItem Customer Detail`
            SET customer_name = ?, modified = NOW(6)
            WHERE name = ?
            "#,
        )
        .bind(customer)
        .bind(name)
        .execute(&direct.pool)
        .await?;
        Ok(())
    }

    async fn cleanup_test_item_customer(direct: &DirectDbReader, name: &str) {
        let _ = sqlx::query("DELETE FROM `tabItem Customer Detail` WHERE name = ?")
            .bind(name)
            .execute(&direct.pool)
            .await;
    }

    async fn cleanup_catalog_prefix(direct: &DirectDbReader, prefix: &str) {
        let like = format!("{prefix}%");
        for sql in [
            "DELETE FROM `tabItem Supplier` WHERE name LIKE ? OR parent LIKE ? OR supplier LIKE ?",
            "DELETE FROM `tabItem Customer Detail` WHERE name LIKE ? OR parent LIKE ? OR customer_name LIKE ?",
        ] {
            let _ = sqlx::query(sql)
                .bind(&like)
                .bind(&like)
                .bind(&like)
                .execute(&direct.pool)
                .await;
        }
        for sql in [
            "DELETE FROM tabItem WHERE name LIKE ?",
            "DELETE FROM `tabItem Group` WHERE name LIKE ?",
            "DELETE FROM tabSupplier WHERE name LIKE ?",
            "DELETE FROM tabCustomer WHERE name LIKE ?",
        ] {
            let _ = sqlx::query(sql).bind(&like).execute(&direct.pool).await;
        }
    }
}
