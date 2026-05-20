use std::cmp::Ordering as CmpOrdering;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup};
use crate::core::profile::ports::{CustomerProfileRecord, SupplierProfileRecord};
use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, SupplierDirectoryEntry, SupplierItem,
};
use crate::erpdb::catalog_cache::schema;
use crate::erpdb::werka_item_search::{
    SupplierItemSearchEntry, rank_customer_item_entries_by_query,
    rank_customer_item_options_by_query, rank_supplier_items_by_query, slice_page,
};
use crate::erpdb::werka_suppliers::clamp_limit;

#[derive(Debug, thiserror::Error)]
pub enum CatalogCacheError {
    #[error("catalog cache not ready")]
    NotReady,
    #[error("catalog cache lock failed")]
    LockFailed,
    #[error("catalog cache sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("catalog cache sync failed: {0}")]
    Sync(String),
    #[error("catalog cache io failed: {0}")]
    Io(#[from] std::io::Error),
}

pub struct CatalogCacheStore {
    conn: Mutex<Connection>,
    ready: AtomicBool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItem {
    pub name: String,
    pub item_name: String,
    pub stock_uom: String,
    pub item_group: String,
    pub modified: String,
    pub disabled: bool,
    pub is_stock_item: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemGroup {
    pub name: String,
    pub item_group_name: String,
    pub parent_item_group: String,
    pub is_group: bool,
    pub lft: i64,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedSupplier {
    pub name: String,
    pub supplier_name: String,
    pub mobile_no: String,
    pub supplier_details: String,
    pub image: String,
    pub disabled: bool,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedCustomer {
    pub name: String,
    pub customer_name: String,
    pub mobile_no: String,
    pub customer_details: String,
    pub disabled: bool,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemSupplier {
    pub parent: String,
    pub supplier: String,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemCustomer {
    pub parent: String,
    pub customer_name: String,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogSnapshot {
    pub items: Vec<CachedItem>,
    pub item_groups: Vec<CachedItemGroup>,
    pub suppliers: Vec<CachedSupplier>,
    pub customers: Vec<CachedCustomer>,
    pub item_suppliers: Vec<CachedItemSupplier>,
    pub item_customers: Vec<CachedItemCustomer>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogKeySnapshot {
    pub items: Option<Vec<String>>,
    pub item_groups: Option<Vec<String>>,
    pub suppliers: Option<Vec<String>>,
    pub customers: Option<Vec<String>>,
    pub item_suppliers: Option<Vec<(String, String)>>,
    pub item_customers: Option<Vec<(String, String)>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogDeltaSnapshot {
    pub changed: CatalogSnapshot,
    pub keys: CatalogKeySnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogTableStats {
    pub count: i64,
    pub max_modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStatsSnapshot {
    pub items: CatalogTableStats,
    pub item_groups: CatalogTableStats,
    pub suppliers: CatalogTableStats,
    pub customers: CatalogTableStats,
    pub item_suppliers: CatalogTableStats,
    pub item_customers: CatalogTableStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatalogMissingChangedKeys {
    pub items: bool,
    pub item_groups: bool,
    pub suppliers: bool,
    pub customers: bool,
    pub item_suppliers: bool,
    pub item_customers: bool,
}

impl CatalogCacheStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CatalogCacheError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        register_catalog_collation(&conn)?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            ready: AtomicBool::new(false),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, CatalogCacheError> {
        let conn = Connection::open_in_memory()?;
        register_catalog_collation(&conn)?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            ready: AtomicBool::new(false),
        })
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn replace_catalog(&self, snapshot: CatalogSnapshot) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            DELETE FROM catalog_item_suppliers;
            DELETE FROM catalog_item_customers;
            DELETE FROM catalog_items;
            DELETE FROM catalog_item_groups;
            DELETE FROM catalog_suppliers;
            DELETE FROM catalog_customers;
            "#,
        )?;
        insert_items(&tx, &snapshot.items)?;
        insert_item_groups(&tx, &snapshot.item_groups)?;
        insert_suppliers(&tx, &snapshot.suppliers)?;
        insert_customers(&tx, &snapshot.customers)?;
        insert_item_suppliers(&tx, &snapshot.item_suppliers)?;
        insert_item_customers(&tx, &snapshot.item_customers)?;
        tx.commit()?;
        self.mark_ready();
        Ok(())
    }

    pub fn apply_delta(&self, delta: CatalogDeltaSnapshot) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_items(&tx, &delta.changed.items)?;
        insert_item_groups(&tx, &delta.changed.item_groups)?;
        insert_suppliers(&tx, &delta.changed.suppliers)?;
        insert_customers(&tx, &delta.changed.customers)?;
        insert_item_suppliers(&tx, &delta.changed.item_suppliers)?;
        insert_item_customers(&tx, &delta.changed.item_customers)?;
        if let Some(keys) = &delta.keys.items {
            retain_single_key_table(&tx, "catalog_items", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.item_groups {
            retain_single_key_table(&tx, "catalog_item_groups", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.suppliers {
            retain_single_key_table(&tx, "catalog_suppliers", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.customers {
            retain_single_key_table(&tx, "catalog_customers", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.item_suppliers {
            retain_composite_key_table(
                &tx,
                "catalog_item_suppliers",
                "parent",
                "supplier",
                "temp_catalog_item_supplier_keys",
                keys,
            )?;
        }
        if let Some(keys) = &delta.keys.item_customers {
            retain_composite_key_table(
                &tx,
                "catalog_item_customers",
                "parent",
                "customer_name",
                "temp_catalog_item_customer_keys",
                keys,
            )?;
        }
        tx.commit()?;
        self.mark_ready();
        Ok(())
    }

    pub fn stats(&self) -> Result<CatalogStatsSnapshot, CatalogCacheError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        Ok(CatalogStatsSnapshot {
            items: table_stats(&conn, "catalog_items")?,
            item_groups: table_stats(&conn, "catalog_item_groups")?,
            suppliers: table_stats(&conn, "catalog_suppliers")?,
            customers: table_stats(&conn, "catalog_customers")?,
            item_suppliers: table_stats(&conn, "catalog_item_suppliers")?,
            item_customers: table_stats(&conn, "catalog_item_customers")?,
        })
    }

    pub fn missing_changed_keys(
        &self,
        changed: &CatalogSnapshot,
    ) -> Result<CatalogMissingChangedKeys, CatalogCacheError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        Ok(CatalogMissingChangedKeys {
            items: single_keys_missing(
                &conn,
                "catalog_items",
                "name",
                changed.items.iter().map(|row| row.name.as_str()),
            )?,
            item_groups: single_keys_missing(
                &conn,
                "catalog_item_groups",
                "name",
                changed.item_groups.iter().map(|row| row.name.as_str()),
            )?,
            suppliers: single_keys_missing(
                &conn,
                "catalog_suppliers",
                "name",
                changed.suppliers.iter().map(|row| row.name.as_str()),
            )?,
            customers: single_keys_missing(
                &conn,
                "catalog_customers",
                "name",
                changed.customers.iter().map(|row| row.name.as_str()),
            )?,
            item_suppliers: composite_keys_missing(
                &conn,
                "catalog_item_suppliers",
                "parent",
                "supplier",
                changed
                    .item_suppliers
                    .iter()
                    .map(|row| (row.parent.as_str(), row.supplier.as_str())),
            )?,
            item_customers: composite_keys_missing(
                &conn,
                "catalog_item_customers",
                "parent",
                "customer_name",
                changed
                    .item_customers
                    .iter()
                    .map(|row| (row.parent.as_str(), row.customer_name.as_str())),
            )?,
        })
    }

    pub fn upsert_items(&self, items: &[CachedItem]) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_items(&tx, items)?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_item_groups(&self, groups: &[CachedItemGroup]) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_item_groups(&tx, groups)?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_suppliers(&self, suppliers: &[CachedSupplier]) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_suppliers(&tx, suppliers)?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_customers(&self, customers: &[CachedCustomer]) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_customers(&tx, customers)?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_item_suppliers(
        &self,
        links: &[CachedItemSupplier],
    ) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_item_suppliers(&tx, links)?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_item_customers(
        &self,
        links: &[CachedItemCustomer],
    ) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_item_customers(&tx, links)?;
        tx.commit()?;
        Ok(())
    }

    pub fn items_page(
        &self,
        query: &str,
        group: Option<&str>,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let group = group.map(str::trim).filter(|value| !value.is_empty());
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let like = sqlite_like_pattern(query);
        let mut stmt = conn.prepare(match group {
            Some(_) => {
                r#"
                SELECT name, item_name, stock_uom, item_group
                FROM catalog_items
                WHERE disabled = 0
                  AND is_stock_item = 1
                  AND item_group = ?1
                  AND (?2 = '' OR name LIKE ?3 ESCAPE '\' OR item_name LIKE ?3 ESCAPE '\')
                ORDER BY item_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
                LIMIT ?4 OFFSET ?5
                "#
            }
            None => {
                r#"
                SELECT name, item_name, stock_uom, item_group
                FROM catalog_items
                WHERE disabled = 0
                  AND is_stock_item = 1
                  AND (?1 = '' OR name LIKE ?2 ESCAPE '\' OR item_name LIKE ?2 ESCAPE '\')
                ORDER BY item_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
                LIMIT ?3 OFFSET ?4
                "#
            }
        })?;
        match group {
            Some(group) => {
                let rows = stmt.query_map(
                    params![group, query.trim(), like, limit as i64, offset as i64],
                    |row| supplier_item_from_row(row, default_warehouse),
                )?;
                collect_rows(rows)
            }
            None => {
                let rows = stmt.query_map(
                    params![query.trim(), like, limit as i64, offset as i64],
                    |row| supplier_item_from_row(row, default_warehouse),
                )?;
                collect_rows(rows)
            }
        }
    }

    pub fn items_by_codes(
        &self,
        item_codes: &[String],
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let codes = item_codes
            .iter()
            .map(|code| code.trim().to_string())
            .filter(|code| !code.is_empty())
            .take(500)
            .collect::<Vec<_>>();
        if codes.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", codes.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT name, item_name, stock_uom, item_group
            FROM catalog_items
            WHERE disabled = 0
              AND is_stock_item = 1
              AND name IN ({placeholders})
            ORDER BY item_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
            "#
        );
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(codes.iter()), |row| {
            supplier_item_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    pub fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name
            FROM catalog_item_groups
            WHERE ?1 = '' OR name LIKE ?2 ESCAPE '\' OR item_group_name LIKE ?2 ESCAPE '\'
            ORDER BY name ASC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(params![query.trim(), like, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        collect_rows(rows).map(|groups| {
            groups
                .into_iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect()
        })
    }

    pub fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, item_group_name, parent_item_group, is_group
            FROM catalog_item_groups
            ORDER BY lft ASC, name ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let item_group_name: String = row.get(1)?;
            Ok(AdminItemGroup {
                name: name.trim().to_string(),
                item_group_name: blank_default(&item_group_name, &name),
                parent_item_group: row.get::<_, String>(2)?.trim().to_string(),
                is_group: row.get::<_, i64>(3)? != 0,
            })
        })?;
        collect_rows(rows)
    }

    pub fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, supplier_name, mobile_no
            FROM catalog_suppliers
            WHERE disabled = 0
              AND (?1 = '' OR name LIKE ?2 ESCAPE '\' OR supplier_name LIKE ?2 ESCAPE '\' OR mobile_no LIKE ?2 ESCAPE '\')
            ORDER BY modified DESC, supplier_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| admin_supplier_from_row(row),
        )?;
        collect_rows(rows)
    }

    pub fn supplier_by_ref(
        &self,
        ref_: &str,
    ) -> Result<Option<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        conn.query_row(
            r#"
            SELECT name, supplier_name, mobile_no
            FROM catalog_suppliers
            WHERE disabled = 0 AND name = ?1
            LIMIT 1
            "#,
            params![ref_.trim()],
            admin_supplier_from_row,
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, customer_name, mobile_no
            FROM catalog_customers
            WHERE disabled = 0
              AND (?1 = '' OR name LIKE ?2 ESCAPE '\' OR customer_name LIKE ?2 ESCAPE '\' OR mobile_no LIKE ?2 ESCAPE '\')
            ORDER BY modified DESC, customer_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| admin_customer_from_row(row),
        )?;
        collect_rows(rows)
    }

    pub fn customer_by_ref(
        &self,
        ref_: &str,
    ) -> Result<Option<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        conn.query_row(
            r#"
            SELECT name, customer_name, mobile_no
            FROM catalog_customers
            WHERE disabled = 0 AND name = ?1
            LIMIT 1
            "#,
            params![ref_.trim()],
            admin_customer_from_row,
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn supplier_profile(
        &self,
        id: &str,
    ) -> Result<Option<SupplierProfileRecord>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        conn.query_row(
            r#"
            SELECT mobile_no, supplier_details, image
            FROM catalog_suppliers
            WHERE name = ?1
            LIMIT 1
            "#,
            params![id.trim()],
            |row| {
                Ok(SupplierProfileRecord {
                    phone: profile_phone(&row.get::<_, String>(0)?, &row.get::<_, String>(1)?),
                    image: row.get::<_, String>(2)?.trim().to_string(),
                })
            },
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn customer_profile(
        &self,
        id: &str,
    ) -> Result<Option<CustomerProfileRecord>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        conn.query_row(
            r#"
            SELECT mobile_no, customer_details
            FROM catalog_customers
            WHERE name = ?1
            LIMIT 1
            "#,
            params![id.trim()],
            |row| {
                Ok(CustomerProfileRecord {
                    phone: profile_phone(&row.get::<_, String>(0)?, &row.get::<_, String>(1)?),
                })
            },
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 200, 500);
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_suppliers isup
            INNER JOIN catalog_items i ON i.name = isup.parent
            WHERE isup.supplier = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![supplier_ref.trim(), limit as i64], |row| {
            supplier_item_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    pub fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        if query.trim().is_empty() {
            return self.customer_items_page(customer_ref, limit, offset, default_warehouse);
        }
        let entries = self.customer_item_search_entries(customer_ref, default_warehouse)?;
        Ok(slice_page(
            &rank_customer_item_entries_by_query(entries, query),
            offset,
            limit,
        ))
    }

    pub fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        let mut items =
            self.customer_items(customer_ref, query, limit, offset, default_warehouse)?;
        clear_item_groups(&mut items);
        Ok(items)
    }

    pub fn supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let mut items = self.supplier_items_all(supplier_ref, default_warehouse)?;
        clear_item_groups(&mut items);
        if query.trim().is_empty() {
            return Ok(slice_page(&items, offset, limit));
        }
        Ok(slice_page(
            &rank_supplier_items_by_query(items, query),
            offset,
            limit,
        ))
    }

    pub fn customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<CustomerItemOption>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let items = self.customer_item_options_all(default_warehouse)?;
        if query.trim().is_empty() {
            return Ok(slice_page(&items, offset, limit));
        }
        Ok(slice_page(
            &rank_customer_item_options_by_query(items, query),
            offset,
            limit,
        ))
    }

    pub fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT s.name, s.supplier_name, s.mobile_no
            FROM catalog_item_suppliers isup
            INNER JOIN catalog_suppliers s ON s.name = isup.supplier
            INNER JOIN catalog_items i ON i.name = isup.parent
            WHERE s.disabled = 0
              AND i.disabled = 0
              AND i.is_stock_item = 1
              AND (?1 = '' OR s.name LIKE ?2 ESCAPE '\' OR s.supplier_name LIKE ?2 ESCAPE '\' OR s.mobile_no LIKE ?2 ESCAPE '\')
            ORDER BY s.modified DESC, s.supplier_name COLLATE ERP_CATALOG ASC, s.name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| {
                Ok(SupplierDirectoryEntry {
                    ref_: row.get::<_, String>(0)?.trim().to_string(),
                    name: blank_default(&row.get::<_, String>(1)?, &row.get::<_, String>(0)?),
                    phone: row.get::<_, String>(2)?.trim().to_string(),
                })
            },
        )?;
        collect_rows(rows)
    }

    pub fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT c.name, c.customer_name, c.mobile_no
            FROM catalog_customers c
            INNER JOIN catalog_item_customers icd ON icd.customer_name = c.name
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE c.disabled = 0
              AND i.disabled = 0
              AND i.is_stock_item = 1
              AND (?1 = '' OR c.name LIKE ?2 ESCAPE '\' OR c.customer_name LIKE ?2 ESCAPE '\' OR c.mobile_no LIKE ?2 ESCAPE '\')
            ORDER BY c.modified DESC, c.customer_name COLLATE ERP_CATALOG ASC, c.name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| {
                Ok(CustomerDirectoryEntry {
                    ref_: row.get::<_, String>(0)?.trim().to_string(),
                    name: blank_default(&row.get::<_, String>(1)?, &row.get::<_, String>(0)?),
                    phone: row.get::<_, String>(2)?.trim().to_string(),
                })
            },
        )?;
        collect_rows(rows)
    }

    fn customer_items_page(
        &self,
        customer_ref: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_customers icd
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE icd.customer_name = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            LIMIT ?2 OFFSET ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![customer_ref.trim(), limit as i64, offset as i64],
            |row| supplier_item_from_row(row, default_warehouse),
        )?;
        collect_rows(rows)
    }

    fn supplier_items_all(
        &self,
        supplier_ref: &str,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_suppliers isup
            INNER JOIN catalog_items i ON i.name = isup.parent
            WHERE isup.supplier = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            "#,
        )?;
        let rows = stmt.query_map(params![supplier_ref.trim()], |row| {
            supplier_item_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    fn customer_item_search_entries(
        &self,
        customer_ref: &str,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItemSearchEntry>, CatalogCacheError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_customers icd
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE icd.customer_name = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            "#,
        )?;
        let rows = stmt.query_map(params![customer_ref.trim()], |row| {
            let item = supplier_item_from_row(row, default_warehouse)?;
            Ok(SupplierItemSearchEntry {
                search_terms: vec![item.code.clone(), item.name.clone()],
                item,
            })
        })?;
        collect_rows(rows)
    }

    fn customer_item_options_all(
        &self,
        default_warehouse: &str,
    ) -> Result<Vec<CustomerItemOption>, CatalogCacheError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT
                c.name,
                c.customer_name,
                c.mobile_no,
                i.name,
                i.item_name,
                i.stock_uom
            FROM catalog_item_customers icd
            INNER JOIN catalog_customers c ON c.name = icd.customer_name
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE c.disabled = 0
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, c.customer_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let customer_ref: String = row.get(0)?;
            let customer_name: String = row.get(1)?;
            let item_code: String = row.get(3)?;
            let item_name: String = row.get(4)?;
            Ok(CustomerItemOption {
                customer_ref: customer_ref.trim().to_string(),
                customer_name: blank_default(&customer_name, &customer_ref),
                customer_phone: row.get::<_, String>(2)?.trim().to_string(),
                item_code: item_code.trim().to_string(),
                item_name: blank_default(&item_name, &item_code),
                uom: row.get::<_, String>(5)?.trim().to_string(),
                warehouse: default_warehouse.trim().to_string(),
            })
        })?;
        collect_rows(rows)
    }

    fn ensure_ready(&self) -> Result<(), CatalogCacheError> {
        if self.ready.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(CatalogCacheError::NotReady)
        }
    }
}

fn clear_item_groups(items: &mut [SupplierItem]) {
    for item in items {
        item.item_group.clear();
    }
}

fn supplier_item_from_row(
    row: &rusqlite::Row<'_>,
    default_warehouse: &str,
) -> rusqlite::Result<SupplierItem> {
    let code: String = row.get(0)?;
    let name: String = row.get(1)?;
    Ok(SupplierItem {
        code: code.trim().to_string(),
        name: blank_default(&name, &code),
        uom: row.get::<_, String>(2)?.trim().to_string(),
        warehouse: default_warehouse.trim().to_string(),
        item_group: row.get::<_, String>(3)?.trim().to_string(),
    })
}

fn insert_items(tx: &rusqlite::Transaction<'_>, items: &[CachedItem]) -> rusqlite::Result<()> {
    for item in items {
        tx.execute(
            r#"
            INSERT INTO catalog_items
                (name, item_name, stock_uom, item_group, modified, disabled, is_stock_item)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(name) DO UPDATE SET
                item_name = excluded.item_name,
                stock_uom = excluded.stock_uom,
                item_group = excluded.item_group,
                modified = excluded.modified,
                disabled = excluded.disabled,
                is_stock_item = excluded.is_stock_item
            "#,
            params![
                item.name.trim(),
                blank_default(&item.item_name, &item.name),
                item.stock_uom.trim(),
                item.item_group.trim(),
                item.modified.trim(),
                bool_int(item.disabled),
                bool_int(item.is_stock_item),
            ],
        )?;
    }
    Ok(())
}

fn insert_item_groups(
    tx: &rusqlite::Transaction<'_>,
    groups: &[CachedItemGroup],
) -> rusqlite::Result<()> {
    for group in groups {
        tx.execute(
            r#"
            INSERT INTO catalog_item_groups
                (name, item_group_name, parent_item_group, is_group, lft, modified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(name) DO UPDATE SET
                item_group_name = excluded.item_group_name,
                parent_item_group = excluded.parent_item_group,
                is_group = excluded.is_group,
                lft = excluded.lft,
                modified = excluded.modified
            "#,
            params![
                group.name.trim(),
                blank_default(&group.item_group_name, &group.name),
                group.parent_item_group.trim(),
                bool_int(group.is_group),
                group.lft,
                group.modified.trim(),
            ],
        )?;
    }
    Ok(())
}

fn insert_suppliers(
    tx: &rusqlite::Transaction<'_>,
    suppliers: &[CachedSupplier],
) -> rusqlite::Result<()> {
    for supplier in suppliers {
        tx.execute(
            r#"
            INSERT INTO catalog_suppliers
                (name, supplier_name, mobile_no, supplier_details, image, disabled, modified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(name) DO UPDATE SET
                supplier_name = excluded.supplier_name,
                mobile_no = excluded.mobile_no,
                supplier_details = excluded.supplier_details,
                image = excluded.image,
                disabled = excluded.disabled,
                modified = excluded.modified
            "#,
            params![
                supplier.name.trim(),
                blank_default(&supplier.supplier_name, &supplier.name),
                supplier.mobile_no.trim(),
                supplier.supplier_details.trim(),
                supplier.image.trim(),
                bool_int(supplier.disabled),
                supplier.modified.trim(),
            ],
        )?;
    }
    Ok(())
}

fn insert_customers(
    tx: &rusqlite::Transaction<'_>,
    customers: &[CachedCustomer],
) -> rusqlite::Result<()> {
    for customer in customers {
        tx.execute(
            r#"
            INSERT INTO catalog_customers
                (name, customer_name, mobile_no, customer_details, disabled, modified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(name) DO UPDATE SET
                customer_name = excluded.customer_name,
                mobile_no = excluded.mobile_no,
                customer_details = excluded.customer_details,
                disabled = excluded.disabled,
                modified = excluded.modified
            "#,
            params![
                customer.name.trim(),
                blank_default(&customer.customer_name, &customer.name),
                customer.mobile_no.trim(),
                customer.customer_details.trim(),
                bool_int(customer.disabled),
                customer.modified.trim(),
            ],
        )?;
    }
    Ok(())
}

fn insert_item_suppliers(
    tx: &rusqlite::Transaction<'_>,
    links: &[CachedItemSupplier],
) -> rusqlite::Result<()> {
    for link in links {
        tx.execute(
            r#"
            INSERT INTO catalog_item_suppliers (parent, supplier, modified)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(parent, supplier) DO UPDATE SET modified = excluded.modified
            "#,
            params![
                link.parent.trim(),
                link.supplier.trim(),
                link.modified.trim()
            ],
        )?;
    }
    Ok(())
}

fn insert_item_customers(
    tx: &rusqlite::Transaction<'_>,
    links: &[CachedItemCustomer],
) -> rusqlite::Result<()> {
    for link in links {
        tx.execute(
            r#"
            INSERT INTO catalog_item_customers (parent, customer_name, modified)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(parent, customer_name) DO UPDATE SET modified = excluded.modified
            "#,
            params![
                link.parent.trim(),
                link.customer_name.trim(),
                link.modified.trim()
            ],
        )?;
    }
    Ok(())
}

fn retain_single_key_table(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    keys: &[String],
) -> rusqlite::Result<()> {
    if keys.is_empty() {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
        return Ok(());
    }
    let placeholders = std::iter::repeat("?")
        .take(keys.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("DELETE FROM {table} WHERE {column} NOT IN ({placeholders})");
    tx.execute(&sql, params_from_iter(keys.iter().map(|key| key.trim())))?;
    Ok(())
}

fn retain_composite_key_table(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    left_column: &str,
    right_column: &str,
    temp_table: &str,
    keys: &[(String, String)],
) -> rusqlite::Result<()> {
    if keys.is_empty() {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
        return Ok(());
    }

    tx.execute(
        &format!(
            "CREATE TEMP TABLE IF NOT EXISTS {temp_table} (left_key TEXT NOT NULL, right_key TEXT NOT NULL, PRIMARY KEY (left_key, right_key))"
        ),
        [],
    )?;
    tx.execute(&format!("DELETE FROM {temp_table}"), [])?;
    for (left, right) in keys {
        tx.execute(
            &format!("INSERT OR IGNORE INTO {temp_table} (left_key, right_key) VALUES (?1, ?2)"),
            params![left.trim(), right.trim()],
        )?;
    }
    tx.execute(
        &format!(
            "DELETE FROM {table}
             WHERE NOT EXISTS (
                 SELECT 1 FROM {temp_table} keys
                 WHERE keys.left_key = {table}.{left_column}
                   AND keys.right_key = {table}.{right_column}
             )"
        ),
        [],
    )?;
    tx.execute(&format!("DELETE FROM {temp_table}"), [])?;
    Ok(())
}

fn table_stats(conn: &Connection, table: &str) -> rusqlite::Result<CatalogTableStats> {
    conn.query_row(
        &format!("SELECT COUNT(*), COALESCE(MAX(modified), '') FROM {table}"),
        [],
        |row| {
            Ok(CatalogTableStats {
                count: row.get(0)?,
                max_modified: row.get(1)?,
            })
        },
    )
}

fn single_keys_missing<'a>(
    conn: &Connection,
    table: &str,
    column: &str,
    keys: impl Iterator<Item = &'a str>,
) -> rusqlite::Result<bool> {
    for key in keys {
        let exists = conn
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE {column} = ?1 LIMIT 1"),
                params![key.trim()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(true);
        }
    }
    Ok(false)
}

fn composite_keys_missing<'a>(
    conn: &Connection,
    table: &str,
    left_column: &str,
    right_column: &str,
    keys: impl Iterator<Item = (&'a str, &'a str)>,
) -> rusqlite::Result<bool> {
    for (left, right) in keys {
        let exists = conn
            .query_row(
                &format!(
                    "SELECT 1 FROM {table} WHERE {left_column} = ?1 AND {right_column} = ?2 LIMIT 1"
                ),
                params![left.trim(), right.trim()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(true);
        }
    }
    Ok(false)
}

fn admin_supplier_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminDirectoryEntry> {
    let ref_: String = row.get(0)?;
    let name: String = row.get(1)?;
    Ok(AdminDirectoryEntry {
        ref_: ref_.trim().to_string(),
        name: blank_default(&name, &ref_),
        phone: row.get::<_, String>(2)?.trim().to_string(),
    })
}

fn admin_customer_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminDirectoryEntry> {
    let ref_: String = row.get(0)?;
    let name: String = row.get(1)?;
    Ok(AdminDirectoryEntry {
        ref_: ref_.trim().to_string(),
        name: blank_default(&name, &ref_),
        phone: row.get::<_, String>(2)?.trim().to_string(),
    })
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, CatalogCacheError> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(CatalogCacheError::from)
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn bool_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn sqlite_like_pattern(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return "%".to_string();
    }
    let escaped = trimmed
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_");
    format!("%{escaped}%")
}

fn register_catalog_collation(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_collation("ERP_CATALOG", |left, right| {
        catalog_sort_key(left).cmp(&catalog_sort_key(right))
    })
}

fn catalog_sort_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace(['’', '‘', 'ʻ', 'ʼ', '`'], "'")
        .replace('ﬁ', "fi")
        .replace('ﬀ', "ff")
        .replace('ﬂ', "fl")
        .replace('ﬃ', "ffi")
        .replace('ﬄ', "ffl")
}

fn profile_phone(mobile_no: &str, details: &str) -> String {
    if !mobile_no.trim().is_empty() {
        return mobile_no.trim().to_string();
    }
    for line in details.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("telefon:") {
            return trimmed["telefon:".len()..].trim().to_string();
        }
        if lower.starts_with("phone:") {
            return trimmed["phone:".len()..].trim().to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_page_searches_groups_and_pages_active_stock_items() {
        let store = seeded_store();

        assert_eq!(
            store
                .items_page("alma", None, 10, 0, "Stores - A")
                .expect("items")
                .into_iter()
                .map(|item| item.code)
                .collect::<Vec<_>>(),
            vec!["ITEM-001"]
        );
        assert_eq!(
            store
                .items_page("", Some("Finished"), 10, 0, "Stores - A")
                .expect("items")
                .into_iter()
                .map(|item| item.code)
                .collect::<Vec<_>>(),
            vec!["ITEM-001", "ITEM-002"]
        );
        assert_eq!(
            store
                .items_page("", None, 1, 1, "Stores - A")
                .expect("items")[0]
                .code,
            "ITEM-002"
        );
    }

    #[test]
    fn item_groups_keep_tree_order() {
        let store = seeded_store();
        let groups = store.item_group_tree().expect("groups");

        assert_eq!(
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            vec!["All Item Groups", "Finished", "Raw"]
        );
        assert_eq!(groups[1].parent_item_group, "All Item Groups");
        assert!(!groups[1].is_group);
    }

    #[test]
    fn supplier_and_customer_mappings_return_items() {
        let store = seeded_store();

        let supplier_items = store
            .supplier_items("SUP-001", "", 20, 0, "Stores - A")
            .expect("supplier items");
        assert_eq!(
            supplier_items
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>(),
            vec!["ITEM-001", "ITEM-002"]
        );

        let customer_items = store
            .customer_items("CUS-001", "nok", 20, 0, "Stores - A")
            .expect("customer items");
        assert_eq!(customer_items[0].code, "ITEM-002");
    }

    #[test]
    fn directory_queries_skip_disabled_records() {
        let store = seeded_store();

        assert_eq!(
            store
                .suppliers_page("", 20, 0)
                .expect("suppliers")
                .into_iter()
                .map(|supplier| supplier.ref_)
                .collect::<Vec<_>>(),
            vec!["SUP-001"]
        );
        assert_eq!(
            store.customers_page("Ali", 20, 0).expect("customers")[0].ref_,
            "CUS-001"
        );
    }

    #[test]
    fn catalog_sort_key_matches_erp_item_order_edge_cases() {
        assert_eq!(
            catalog_sort_key("A’lo Ta’m Kanada").cmp(&catalog_sort_key("ABCD Family")),
            CmpOrdering::Less
        );
        assert_eq!(
            catalog_sort_key("Almond ﬁstashka paket")
                .cmp(&catalog_sort_key("Almond qurt samarqand paket")),
            CmpOrdering::Less
        );
    }

    #[test]
    fn replace_catalog_removes_rows_missing_from_latest_snapshot() {
        let store = seeded_store();

        store
            .replace_catalog(CatalogSnapshot {
                items: vec![CachedItem {
                    name: "ITEM-004".to_string(),
                    item_name: "New Item".to_string(),
                    stock_uom: "Kg".to_string(),
                    item_group: "Finished".to_string(),
                    modified: "2026-05-20 09:00:00".to_string(),
                    disabled: false,
                    is_stock_item: true,
                }],
                item_groups: vec![CachedItemGroup {
                    name: "Finished".to_string(),
                    item_group_name: "Finished".to_string(),
                    parent_item_group: "All Item Groups".to_string(),
                    is_group: false,
                    lft: 1,
                    modified: String::new(),
                }],
                suppliers: vec![CachedSupplier {
                    name: "SUP-003".to_string(),
                    supplier_name: "New Supplier".to_string(),
                    mobile_no: "+99893".to_string(),
                    supplier_details: String::new(),
                    image: String::new(),
                    disabled: false,
                    modified: String::new(),
                }],
                customers: vec![CachedCustomer {
                    name: "CUS-003".to_string(),
                    customer_name: "New Customer".to_string(),
                    mobile_no: "+99894".to_string(),
                    customer_details: String::new(),
                    disabled: false,
                    modified: String::new(),
                }],
                item_suppliers: vec![CachedItemSupplier {
                    parent: "ITEM-004".to_string(),
                    supplier: "SUP-003".to_string(),
                    modified: String::new(),
                }],
                item_customers: vec![CachedItemCustomer {
                    parent: "ITEM-004".to_string(),
                    customer_name: "CUS-003".to_string(),
                    modified: String::new(),
                }],
            })
            .expect("replace catalog");

        assert_eq!(
            store
                .items_page("", None, 20, 0, "Stores - A")
                .expect("items")
                .into_iter()
                .map(|item| item.code)
                .collect::<Vec<_>>(),
            vec!["ITEM-004"]
        );
        assert!(
            store
                .supplier_by_ref("SUP-001")
                .expect("supplier")
                .is_none()
        );
        assert_eq!(
            store
                .supplier_items("SUP-003", "", 20, 0, "Stores - A")
                .expect("supplier items")[0]
                .code,
            "ITEM-004"
        );
    }

    fn seeded_store() -> CatalogCacheStore {
        let store = CatalogCacheStore::in_memory().expect("store");
        store
            .upsert_items(&[
                CachedItem {
                    name: "ITEM-001".to_string(),
                    item_name: "Alma".to_string(),
                    stock_uom: "Kg".to_string(),
                    item_group: "Finished".to_string(),
                    modified: "2026-05-20 08:00:00".to_string(),
                    disabled: false,
                    is_stock_item: true,
                },
                CachedItem {
                    name: "ITEM-002".to_string(),
                    item_name: "Nok".to_string(),
                    stock_uom: "Kg".to_string(),
                    item_group: "Finished".to_string(),
                    modified: "2026-05-20 08:01:00".to_string(),
                    disabled: false,
                    is_stock_item: true,
                },
                CachedItem {
                    name: "ITEM-003".to_string(),
                    item_name: "Disabled".to_string(),
                    stock_uom: "Kg".to_string(),
                    item_group: "Raw".to_string(),
                    modified: "2026-05-20 08:02:00".to_string(),
                    disabled: true,
                    is_stock_item: true,
                },
            ])
            .expect("items");
        store
            .upsert_item_groups(&[
                CachedItemGroup {
                    name: "All Item Groups".to_string(),
                    item_group_name: "All Item Groups".to_string(),
                    parent_item_group: String::new(),
                    is_group: true,
                    lft: 1,
                    modified: String::new(),
                },
                CachedItemGroup {
                    name: "Finished".to_string(),
                    item_group_name: "Finished".to_string(),
                    parent_item_group: "All Item Groups".to_string(),
                    is_group: false,
                    lft: 2,
                    modified: String::new(),
                },
                CachedItemGroup {
                    name: "Raw".to_string(),
                    item_group_name: "Raw".to_string(),
                    parent_item_group: "All Item Groups".to_string(),
                    is_group: false,
                    lft: 3,
                    modified: String::new(),
                },
            ])
            .expect("groups");
        store
            .upsert_suppliers(&[
                CachedSupplier {
                    name: "SUP-001".to_string(),
                    supplier_name: "Best Supplier".to_string(),
                    mobile_no: "+99890".to_string(),
                    supplier_details: String::new(),
                    image: "/files/supplier.png".to_string(),
                    disabled: false,
                    modified: "2026-05-20 08:02:00".to_string(),
                },
                CachedSupplier {
                    name: "SUP-002".to_string(),
                    supplier_name: "Blocked Supplier".to_string(),
                    mobile_no: String::new(),
                    supplier_details: String::new(),
                    image: String::new(),
                    disabled: true,
                    modified: "2026-05-20 08:03:00".to_string(),
                },
            ])
            .expect("suppliers");
        store
            .upsert_customers(&[CachedCustomer {
                name: "CUS-001".to_string(),
                customer_name: "Ali Market".to_string(),
                mobile_no: "+99891".to_string(),
                customer_details: String::new(),
                disabled: false,
                modified: String::new(),
            }])
            .expect("customers");
        store
            .upsert_item_suppliers(&[
                CachedItemSupplier {
                    parent: "ITEM-001".to_string(),
                    supplier: "SUP-001".to_string(),
                    modified: String::new(),
                },
                CachedItemSupplier {
                    parent: "ITEM-002".to_string(),
                    supplier: "SUP-001".to_string(),
                    modified: String::new(),
                },
            ])
            .expect("item suppliers");
        store
            .upsert_item_customers(&[
                CachedItemCustomer {
                    parent: "ITEM-001".to_string(),
                    customer_name: "CUS-001".to_string(),
                    modified: String::new(),
                },
                CachedItemCustomer {
                    parent: "ITEM-002".to_string(),
                    customer_name: "CUS-001".to_string(),
                    modified: String::new(),
                },
            ])
            .expect("item customers");
        store.mark_ready();
        store
    }
}
