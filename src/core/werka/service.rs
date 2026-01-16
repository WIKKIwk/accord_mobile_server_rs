use std::sync::Arc;

use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, SupplierDirectoryEntry,
    SupplierItem, WerkaArchiveResponse, WerkaCustomerIssueBatchLineResult,
    WerkaCustomerIssueBatchResult, WerkaCustomerIssueCreateInput, WerkaCustomerIssueRecord,
    WerkaCustomerIssueSource, WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{
    CreateDeliveryNoteInput, CreatePurchaseReceiptInput, CustomerIssueSourceLookup,
    DeliveryNoteStateUpdate, NotificationDetailWriter, SupplierUnannouncedWriter,
    WerkaCustomerIssueWriter, WerkaHomeLookup, WerkaPortError, WerkaSupplierAdminStateLookup,
    WerkaUnannouncedWriter,
};
use crate::core::werka::unannounced::{
    format_notification_comment, purchase_receipt_to_dispatch_record, supplier_admin_state,
    upsert_werka_unannounced_in_remarks, validate_unannounced_supplier_item,
};

const DELIVERY_FLOW_STATE_SUBMITTED: i32 = 1;
const CUSTOMER_STATE_PENDING: i32 = 1;
const DELIVERY_ACTOR_WERKA: i32 = 1;
const CUSTOMER_ISSUE_SOURCE_MARKER_PREFIX: &str = "accord_customer_issue_source:";

#[derive(Clone, Default)]
pub struct WerkaService {
    lookup: Option<Arc<dyn WerkaHomeLookup>>,
    customer_issue_writer: Option<Arc<dyn WerkaCustomerIssueWriter>>,
    customer_issue_source_lookup: Option<Arc<dyn CustomerIssueSourceLookup>>,
    unannounced_writer: Option<Arc<dyn WerkaUnannouncedWriter>>,
    pub(crate) supplier_unannounced_writer: Option<Arc<dyn SupplierUnannouncedWriter>>,
    pub(crate) notification_detail_writer: Option<Arc<dyn NotificationDetailWriter>>,
    supplier_admin_state_lookup: Option<Arc<dyn WerkaSupplierAdminStateLookup>>,
}

impl WerkaService {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn with_lookup(mut self, lookup: Arc<dyn WerkaHomeLookup>) -> Self {
        self.lookup = Some(lookup);
        self
    }

    #[allow(dead_code)]
    pub fn with_customer_issue_writer(mut self, writer: Arc<dyn WerkaCustomerIssueWriter>) -> Self {
        self.customer_issue_writer = Some(writer);
        self
    }

    #[allow(dead_code)]
    pub fn with_customer_issue_source_lookup(
        mut self,
        lookup: Arc<dyn CustomerIssueSourceLookup>,
    ) -> Self {
        self.customer_issue_source_lookup = Some(lookup);
        self
    }

    #[allow(dead_code)]
    pub fn with_unannounced_writer(mut self, writer: Arc<dyn WerkaUnannouncedWriter>) -> Self {
        self.unannounced_writer = Some(writer);
        self
    }

    pub fn with_supplier_unannounced_writer(
        mut self,
        writer: Arc<dyn SupplierUnannouncedWriter>,
    ) -> Self {
        self.supplier_unannounced_writer = Some(writer);
        self
    }

    pub fn with_notification_detail_writer(
        mut self,
        writer: Arc<dyn NotificationDetailWriter>,
    ) -> Self {
        self.notification_detail_writer = Some(writer);
        self
    }

    pub fn with_supplier_admin_state_lookup(
        mut self,
        lookup: Arc<dyn WerkaSupplierAdminStateLookup>,
    ) -> Self {
        self.supplier_admin_state_lookup = Some(lookup);
        self
    }

    pub async fn home(
        &self,
        pending_limit: usize,
    ) -> Result<Option<WerkaHomeData>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_home(pending_limit).await.map(Some)
    }

    pub async fn summary(&self) -> Result<Option<WerkaHomeSummary>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_summary().await.map(Some)
    }

    pub async fn pending(
        &self,
        limit: usize,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_pending(limit).await.map(Some)
    }

    pub async fn history(&self) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_history().await.map(Some)
    }

    pub async fn status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Option<Vec<WerkaStatusBreakdownEntry>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_status_breakdown(kind).await.map(Some)
    }

    pub async fn status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_status_details(kind, supplier_ref)
            .await
            .map(Some)
    }

    pub async fn archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<Option<WerkaArchiveResponse>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_archive(kind, period, from, to).await.map(Some)
    }

    pub async fn suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<SupplierDirectoryEntry>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_suppliers(query, limit, offset).await.map(Some)
    }

    pub async fn customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<CustomerDirectoryEntry>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_customers(query, limit, offset).await.map(Some)
    }

    pub async fn supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<SupplierItem>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_supplier_items(supplier_ref, query, limit, offset)
            .await
            .map(Some)
    }

    pub async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<SupplierItem>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_customer_items(customer_ref, query, limit, offset)
            .await
            .map(Some)
    }

    pub async fn customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Option<Vec<CustomerItemOption>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup
            .werka_customer_item_options(query, limit, offset)
            .await
            .map(Some)
    }

    pub async fn create_customer_issue(
        &self,
        input: WerkaCustomerIssueCreateInput,
    ) -> Result<Option<WerkaCustomerIssueRecord>, WerkaPortError> {
        let Some(writer) = &self.customer_issue_writer else {
            return Ok(None);
        };

        let customer_ref = input.customer_ref.trim().to_string();
        let item_code = input.item_code.trim().to_string();
        let source = normalize_customer_issue_source(input.source);
        let marker = customer_issue_source_marker(&source);

        if !marker.is_empty() {
            let duplicate = if let Some(lookup) = &self.customer_issue_source_lookup {
                lookup.customer_issue_source_exists(&marker).await?
            } else {
                writer
                    .customer_issue_source_exists_by_scan(&customer_ref, &marker)
                    .await?
            };
            if duplicate {
                return Err(WerkaPortError::DuplicateCustomerIssueSource);
            }
        }

        let items = writer
            .get_items_by_codes(std::slice::from_ref(&item_code))
            .await?;
        let Some(item) = items.into_iter().next() else {
            return Err(WerkaPortError::WriteFailed("item topilmadi".to_string()));
        };
        let warehouse = writer.resolve_warehouse().await?;
        let company = writer.resolve_company().await?;
        let delivery_note = writer
            .create_draft_delivery_note(CreateDeliveryNoteInput {
                customer: customer_ref.clone(),
                company,
                warehouse,
                item_code: item_code.clone(),
                qty: input.qty,
                uom: item.uom.clone(),
                source_key: marker,
            })
            .await?;

        if let Err(error) = writer
            .update_delivery_note_state(
                &delivery_note,
                DeliveryNoteStateUpdate {
                    flow_state: DELIVERY_FLOW_STATE_SUBMITTED.to_string(),
                    customer_state: CUSTOMER_STATE_PENDING.to_string(),
                    customer_reason: String::new(),
                    delivery_actor: DELIVERY_ACTOR_WERKA.to_string(),
                    ui_status: customer_delivery_ui_status(
                        DELIVERY_FLOW_STATE_SUBMITTED,
                        CUSTOMER_STATE_PENDING,
                    )
                    .to_string(),
                },
            )
            .await
        {
            let _ = writer.delete_delivery_note(&delivery_note).await;
            return Err(error);
        }

        if let Err(error) = writer.submit_delivery_note(&delivery_note).await {
            let _ = writer.delete_delivery_note(&delivery_note).await;
            return Err(error);
        }

        Ok(Some(WerkaCustomerIssueRecord {
            entry_id: delivery_note,
            customer_ref: customer_ref.clone(),
            customer_name: customer_ref,
            item_code: item.code,
            item_name: item.name,
            uom: item.uom,
            qty: input.qty,
            created_label: current_timestamp_label(),
        }))
    }

    pub async fn create_customer_issue_batch(
        &self,
        client_batch_id: &str,
        lines: Vec<WerkaCustomerIssueCreateInput>,
    ) -> Result<Option<WerkaCustomerIssueBatchResult>, WerkaPortError> {
        if self.customer_issue_writer.is_none() {
            return Ok(None);
        }

        let mut created = Vec::new();
        let mut failed = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            match self.create_customer_issue(line).await {
                Ok(Some(record)) => created.push(WerkaCustomerIssueBatchLineResult {
                    line_index: index,
                    record: Some(record),
                    ..WerkaCustomerIssueBatchLineResult::default()
                }),
                Ok(None) => failed.push(default_batch_failure(index)),
                Err(WerkaPortError::InsufficientStock) => {
                    failed.push(WerkaCustomerIssueBatchLineResult {
                        line_index: index,
                        error: "insufficient stock".to_string(),
                        error_code: "insufficient_stock".to_string(),
                        ..WerkaCustomerIssueBatchLineResult::default()
                    });
                }
                Err(_) => failed.push(default_batch_failure(index)),
            }
        }

        Ok(Some(WerkaCustomerIssueBatchResult {
            client_batch_id: client_batch_id.trim().to_string(),
            created,
            failed,
        }))
    }

    pub async fn create_werka_unannounced_draft(
        &self,
        supplier_ref: &str,
        item_code: &str,
        qty: f64,
        werka_display_name: &str,
    ) -> Result<Option<DispatchRecord>, WerkaPortError> {
        let Some(writer) = &self.unannounced_writer else {
            return Ok(None);
        };

        let supplier = writer.find_supplier_for_werka(supplier_ref).await?;
        let supplier_state =
            supplier_admin_state(self.supplier_admin_state_lookup.as_ref(), &supplier.id).await?;
        if supplier_state.removed || supplier_state.blocked {
            return Err(WerkaPortError::WriteFailed(
                "invalid supplier credentials".to_string(),
            ));
        }
        validate_unannounced_supplier_item(
            self.lookup.as_ref(),
            writer.as_ref(),
            &supplier.id,
            item_code,
            &supplier_state,
        )
        .await?;
        let warehouse = writer.resolve_warehouse().await?;
        let mut draft = writer
            .create_draft_purchase_receipt(CreatePurchaseReceiptInput {
                supplier: supplier.id.clone(),
                supplier_phone: supplier.phone.clone(),
                item_code: item_code.trim().to_string(),
                qty,
                warehouse,
                ..CreatePurchaseReceiptInput::default()
            })
            .await?;

        let remarks = upsert_werka_unannounced_in_remarks(&draft.remarks, "pending", "");
        writer
            .update_purchase_receipt_remarks(&draft.name, &remarks)
            .await?;
        let comment = format_notification_comment(
            "Werka",
            werka_display_name,
            "Aytilmagan mol sifatida qayd qilindi.",
        );
        let _ = writer
            .add_purchase_receipt_comment(&draft.name, &comment)
            .await;

        draft.remarks = remarks;
        let mut record = purchase_receipt_to_dispatch_record(draft, &supplier.name);
        record.event_type = "werka_unannounced_pending".to_string();
        record.highlight = "Werka siz qayd etmagan mahsulotni qabul qildi".to_string();
        Ok(Some(record))
    }
}

fn default_batch_failure(index: usize) -> WerkaCustomerIssueBatchLineResult {
    WerkaCustomerIssueBatchLineResult {
        line_index: index,
        error: "werka customer issue create failed".to_string(),
        ..WerkaCustomerIssueBatchLineResult::default()
    }
}

pub(crate) fn normalize_customer_issue_source(
    source: WerkaCustomerIssueSource,
) -> WerkaCustomerIssueSource {
    WerkaCustomerIssueSource {
        barcode: source.barcode.trim().to_string(),
        stock_entry_name: source.stock_entry_name.trim().to_string(),
        line_index: source.line_index.filter(|value| *value >= 0),
    }
}

pub(crate) fn customer_issue_source_marker(source: &WerkaCustomerIssueSource) -> String {
    let source = normalize_customer_issue_source(source.clone());
    let mut parts = Vec::new();
    if !source.barcode.is_empty() {
        parts.push(format!("source_barcode={}", source.barcode));
    }
    if !source.stock_entry_name.is_empty() {
        parts.push(format!("source_stock_entry={}", source.stock_entry_name));
    }
    if let Some(line_index) = source.line_index {
        parts.push(format!("source_line_index={line_index}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{CUSTOMER_ISSUE_SOURCE_MARKER_PREFIX}{}", parts.join(";"))
    }
}

fn customer_delivery_ui_status(flow_state: i32, customer_state: i32) -> &'static str {
    if flow_state != DELIVERY_FLOW_STATE_SUBMITTED {
        return "pending";
    }
    match customer_state {
        3 => "confirm",
        4 => "partial",
        2 => "rejected",
        _ => "pending",
    }
}

fn current_timestamp_label() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
