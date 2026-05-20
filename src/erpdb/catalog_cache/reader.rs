use std::sync::Arc;

use async_trait::async_trait;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup};
use crate::core::admin::ports::{AdminPortError, AdminReadPort};
use crate::core::profile::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileLookup, ProfilePortError, SupplierProfileRecord,
};
use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, StockEntryBarcodeEntry,
    SupplierDirectoryEntry, SupplierItem, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::erpdb::catalog_cache::store::{CatalogCacheError, CatalogCacheStore};
use crate::erpdb::reader::DirectDbReader;
use time::Date;

#[derive(Clone)]
pub struct CatalogCacheReader {
    store: Arc<CatalogCacheStore>,
    fallback: Option<Arc<DirectDbReader>>,
    default_warehouse: String,
}

impl CatalogCacheReader {
    pub fn new(store: Arc<CatalogCacheStore>, default_warehouse: impl Into<String>) -> Self {
        Self {
            store,
            fallback: None,
            default_warehouse: default_warehouse.into(),
        }
    }

    pub fn with_fallback(mut self, fallback: Arc<DirectDbReader>) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

#[async_trait]
impl AdminReadPort for CatalogCacheReader {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        match self.store.suppliers_page(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .suppliers_page(query, limit, offset)
                    .await
            }
        }
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        match self.store.supplier_by_ref(ref_) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(AdminPortError::NotFound),
            Err(_) => self.fallback_admin()?.supplier_by_ref(ref_).await,
        }
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        match self.store.customers_page(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .customers_page(query, limit, offset)
                    .await
            }
        }
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        match self.store.customer_by_ref(ref_) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(AdminPortError::NotFound),
            Err(_) => self.fallback_admin()?.customer_by_ref(ref_).await,
        }
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .items_page(query, None, limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .items_page(query, limit, offset)
                    .await
            }
        }
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .items_page(query, Some(group), limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .items_page_by_group(group, query, limit, offset)
                    .await
            }
        }
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .items_by_codes(item_codes, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => self.fallback_admin()?.items_by_codes(item_codes).await,
        }
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        match self.store.item_groups(query, limit) {
            Ok(value) => Ok(value),
            Err(_) => self.fallback_admin()?.item_groups(query, limit).await,
        }
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        match self.store.item_group_tree() {
            Ok(value) => Ok(value),
            Err(_) => self.fallback_admin()?.item_group_tree().await,
        }
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .assigned_supplier_items(supplier_ref, limit, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_admin()?
                    .assigned_supplier_items(supplier_ref, limit)
                    .await
            }
        }
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        match self
            .store
            .customer_items(customer_ref, query, limit, 0, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                <DirectDbReader as AdminReadPort>::customer_items(
                    self.fallback_admin()?,
                    customer_ref,
                    query,
                    limit,
                )
                .await
            }
        }
    }
}

#[async_trait]
impl WerkaHomeLookup for CatalogCacheReader {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        self.fallback_werka()?.werka_summary().await
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        self.fallback_werka()?.werka_home(pending_limit).await
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.fallback_werka()?.werka_pending(limit).await
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.fallback_werka()?.werka_history().await
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        self.fallback_werka()?.werka_status_breakdown(kind).await
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.fallback_werka()?
            .werka_status_details(kind, supplier_ref)
            .await
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        self.fallback_werka()?
            .werka_archive(kind, period, from, to)
            .await
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        match self.store.werka_suppliers(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_suppliers(query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        match self.store.werka_customers(query, limit, offset) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_customers(query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        match self
            .store
            .supplier_items(supplier_ref, query, limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_supplier_items(supplier_ref, query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        match self.store.werka_customer_items(
            customer_ref,
            query,
            limit,
            offset,
            &self.default_warehouse,
        ) {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_customer_items(customer_ref, query, limit, offset)
                    .await
            }
        }
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        match self
            .store
            .customer_item_options(query, limit, offset, &self.default_warehouse)
        {
            Ok(value) => Ok(value),
            Err(_) => {
                self.fallback_werka()?
                    .werka_customer_item_options(query, limit, offset)
                    .await
            }
        }
    }

    async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, WerkaPortError> {
        <DirectDbReader as WerkaHomeLookup>::stock_entries_by_barcode(
            self.fallback_werka()?,
            barcode,
            limit,
        )
        .await
    }
}

#[async_trait]
impl ProfileLookup for CatalogCacheReader {
    async fn get_supplier_profile(
        &self,
        id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        match self.store.supplier_profile(id) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(ProfilePortError::LookupFailed),
            Err(_) => self.fallback_profile()?.get_supplier_profile(id).await,
        }
    }

    async fn get_customer_profile(
        &self,
        id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        match self.store.customer_profile(id) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(ProfilePortError::LookupFailed),
            Err(_) => self.fallback_profile()?.get_customer_profile(id).await,
        }
    }

    async fn download_file(&self, file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        self.fallback_profile()?.download_file(file_url).await
    }

    async fn upload_supplier_image(
        &self,
        supplier_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        self.fallback_profile()?
            .upload_supplier_image(supplier_id, filename, content_type, content)
            .await
    }
}

impl CatalogCacheReader {
    fn fallback_admin(&self) -> Result<&DirectDbReader, AdminPortError> {
        self.fallback.as_deref().ok_or(AdminPortError::LookupFailed)
    }

    fn fallback_werka(&self) -> Result<&DirectDbReader, WerkaPortError> {
        self.fallback
            .as_deref()
            .ok_or(WerkaPortError::DirectDbLookupUnavailable)
    }

    fn fallback_profile(&self) -> Result<&DirectDbReader, ProfilePortError> {
        self.fallback
            .as_deref()
            .ok_or(ProfilePortError::LookupFailed)
    }
}

impl From<CatalogCacheError> for AdminPortError {
    fn from(_value: CatalogCacheError) -> Self {
        AdminPortError::LookupFailed
    }
}

impl From<CatalogCacheError> for WerkaPortError {
    fn from(_value: CatalogCacheError) -> Self {
        WerkaPortError::DirectDbLookupUnavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erpdb::catalog_cache::store::{
        CachedCustomer, CachedItem, CachedItemCustomer, CachedItemGroup, CachedItemSupplier,
        CachedSupplier,
    };

    #[tokio::test]
    async fn admin_reader_serves_items_and_groups_from_cache() {
        let reader = seeded_reader();

        let items = reader
            .items_page_by_group("Finished", "", 20, 0)
            .await
            .expect("items");
        assert_eq!(items[0].code, "ITEM-001");
        assert_eq!(items[0].warehouse, "Stores - A");

        let groups = reader.item_group_tree().await.expect("groups");
        assert_eq!(groups[0].name, "All Item Groups");
        assert_eq!(groups[1].parent_item_group, "All Item Groups");
    }

    #[tokio::test]
    async fn werka_reader_serves_directories_and_item_options_from_cache() {
        let reader = seeded_reader();

        let suppliers = reader.werka_suppliers("", 20, 0).await.expect("suppliers");
        assert_eq!(suppliers[0].ref_, "SUP-001");

        let options = reader
            .werka_customer_item_options("alma", 20, 0)
            .await
            .expect("options");
        assert_eq!(options[0].customer_ref, "CUS-001");
        assert_eq!(options[0].item_code, "ITEM-001");
    }

    #[tokio::test]
    async fn profile_reader_serves_phone_and_image_from_cache() {
        let reader = seeded_reader();

        let supplier = reader
            .get_supplier_profile("SUP-001")
            .await
            .expect("supplier profile");
        assert_eq!(supplier.phone, "+99890");
        assert_eq!(supplier.image, "/files/supplier.png");

        let customer = reader
            .get_customer_profile("CUS-001")
            .await
            .expect("customer profile");
        assert_eq!(customer.phone, "+99891");
    }

    fn seeded_reader() -> CatalogCacheReader {
        let store = Arc::new(CatalogCacheStore::in_memory().expect("store"));
        store
            .upsert_items(&[CachedItem {
                name: "ITEM-001".to_string(),
                item_name: "Alma".to_string(),
                stock_uom: "Kg".to_string(),
                item_group: "Finished".to_string(),
                modified: String::new(),
                disabled: false,
                is_stock_item: true,
            }])
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
            ])
            .expect("groups");
        store
            .upsert_suppliers(&[CachedSupplier {
                name: "SUP-001".to_string(),
                supplier_name: "Best Supplier".to_string(),
                mobile_no: "+99890".to_string(),
                supplier_details: String::new(),
                image: "/files/supplier.png".to_string(),
                disabled: false,
                modified: String::new(),
            }])
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
            .upsert_item_suppliers(&[CachedItemSupplier {
                parent: "ITEM-001".to_string(),
                supplier: "SUP-001".to_string(),
                modified: String::new(),
            }])
            .expect("item suppliers");
        store
            .upsert_item_customers(&[CachedItemCustomer {
                parent: "ITEM-001".to_string(),
                customer_name: "CUS-001".to_string(),
                modified: String::new(),
            }])
            .expect("item customers");
        store.mark_ready();

        CatalogCacheReader::new(store, "Stores - A")
    }
}
