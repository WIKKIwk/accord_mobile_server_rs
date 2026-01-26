use std::collections::{HashMap, HashSet};

use crate::core::werka::models::{
    DispatchRecord, SupplierHomeSummary, SupplierStatusBreakdownEntry,
};
use crate::core::werka::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierPurchaseReceiptLookup, WerkaPortError,
};
use crate::core::werka::service::WerkaService;
use crate::core::werka::unannounced::purchase_receipt_to_dispatch_record;

const SUPPLIER_RECEIPT_PAGE_SIZE: usize = 200;

impl WerkaService {
    pub async fn supplier_summary(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
    ) -> Result<Option<SupplierHomeSummary>, WerkaPortError> {
        if let Some(lookup) = &self.supplier_read_lookup {
            return lookup.supplier_summary(supplier_ref).await.map(Some);
        }
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        Ok(Some(build_supplier_summary_from_receipts(
            receipts,
            supplier_display_name,
        )))
    }

    pub async fn supplier_history(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        if let Some(lookup) = &self.supplier_read_lookup {
            return lookup.supplier_history(supplier_ref).await.map(Some);
        }
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        let comments_by_receipt =
            purchase_receipt_comments_by_name(lookup.as_ref(), &receipts).await?;
        Ok(Some(build_supplier_history_from_receipts(
            receipts,
            &comments_by_receipt,
            supplier_display_name,
        )))
    }

    pub async fn supplier_status_breakdown(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
        kind: &str,
    ) -> Result<Option<Vec<SupplierStatusBreakdownEntry>>, WerkaPortError> {
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        Ok(Some(build_supplier_status_breakdown_from_receipts(
            receipts,
            supplier_display_name,
            kind,
        )))
    }

    pub async fn supplier_status_details(
        &self,
        supplier_ref: &str,
        supplier_display_name: &str,
        kind: &str,
        item_code: &str,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.supplier_purchase_receipt_lookup else {
            return Ok(None);
        };

        let receipts = collect_supplier_purchase_receipts(lookup.as_ref(), supplier_ref).await?;
        Ok(Some(build_supplier_status_details_from_receipts(
            receipts,
            supplier_display_name,
            kind,
            item_code,
        )))
    }
}

async fn collect_supplier_purchase_receipts(
    lookup: &dyn SupplierPurchaseReceiptLookup,
    supplier_ref: &str,
) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError> {
    let mut result = Vec::with_capacity(SUPPLIER_RECEIPT_PAGE_SIZE);
    let mut seen = HashSet::with_capacity(SUPPLIER_RECEIPT_PAGE_SIZE);
    let mut offset = 0;
    loop {
        let items = lookup
            .list_supplier_purchase_receipts_page(supplier_ref, SUPPLIER_RECEIPT_PAGE_SIZE, offset)
            .await?;
        for item in &items {
            let name = item.name.trim();
            if !name.is_empty() && seen.insert(name.to_string()) {
                result.push(item.clone());
            }
        }
        if items.len() < SUPPLIER_RECEIPT_PAGE_SIZE {
            return Ok(result);
        }
        offset += SUPPLIER_RECEIPT_PAGE_SIZE;
    }
}

async fn purchase_receipt_comments_by_name(
    lookup: &dyn SupplierPurchaseReceiptLookup,
    receipts: &[PurchaseReceiptDraft],
) -> Result<HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError> {
    let mut names = Vec::new();
    let mut seen = HashSet::with_capacity(receipts.len());
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt.clone(), &receipt.supplier_name);
        if !dispatch_record_needs_comment_scan(&record) {
            continue;
        }
        let name = receipt.name.trim();
        if !name.is_empty() && seen.insert(name.to_string()) {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    lookup
        .list_supplier_purchase_receipt_comments_batch(&names, 100)
        .await
}

fn build_supplier_summary_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
) -> SupplierHomeSummary {
    let mut summary = SupplierHomeSummary::default();
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        match record.status.as_str() {
            "pending" | "draft" => summary.pending_count += 1,
            "accepted" => summary.submitted_count += 1,
            "partial" | "rejected" | "cancelled" => summary.returned_count += 1,
            _ => {}
        }
    }
    summary
}

fn build_supplier_history_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    comments_by_receipt: &HashMap<String, Vec<PurchaseReceiptComment>>,
    supplier_display_name: &str,
) -> Vec<DispatchRecord> {
    receipts
        .into_iter()
        .map(|receipt| {
            let mut record =
                purchase_receipt_to_dispatch_record(receipt.clone(), supplier_display_name);
            for comment in comments_by_receipt
                .get(receipt.name.trim())
                .into_iter()
                .flatten()
            {
                if !is_supplier_acknowledgment_comment(&comment.content) {
                    continue;
                }
                if !record.note.contains("Supplier tasdiqladi:") {
                    if !record.note.trim().is_empty() {
                        record.note.push('\n');
                    }
                    record.note.push_str(
                        "Supplier tasdiqladi: Tasdiqlayman, shu holat bo‘lganini ko‘rdim.",
                    );
                }
                break;
            }
            record
        })
        .collect()
}

fn build_supplier_status_breakdown_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
    kind: &str,
) -> Vec<SupplierStatusBreakdownEntry> {
    let mut grouped = HashMap::<String, SupplierStatusBreakdownEntry>::new();
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        if !record_matches_supplier_breakdown(&record, kind) {
            continue;
        }
        let key = if record.item_code.trim().is_empty() {
            record.item_name.trim().to_string()
        } else {
            record.item_code.trim().to_string()
        };
        let entry = grouped
            .entry(key)
            .or_insert_with(|| SupplierStatusBreakdownEntry {
                item_code: record.item_code.clone(),
                item_name: record.item_name.clone(),
                uom: record.uom.clone(),
                ..SupplierStatusBreakdownEntry::default()
            });
        entry.receipt_count += 1;
        entry.total_sent_qty += record.sent_qty;
        entry.total_accepted_qty += record.accepted_qty;
        entry.total_returned_qty += (record.sent_qty - record.accepted_qty).max(0.0);
        if entry.uom.trim().is_empty() {
            entry.uom = record.uom;
        }
    }

    let mut result = grouped.into_values().collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right.receipt_count.cmp(&left.receipt_count).then_with(|| {
            left.item_name
                .to_lowercase()
                .cmp(&right.item_name.to_lowercase())
        })
    });
    result
}

fn build_supplier_status_details_from_receipts(
    receipts: Vec<PurchaseReceiptDraft>,
    supplier_display_name: &str,
    kind: &str,
    item_code: &str,
) -> Vec<DispatchRecord> {
    let needle = item_code.trim();
    let mut result = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        let record = purchase_receipt_to_dispatch_record(receipt, supplier_display_name);
        if !record_matches_supplier_breakdown(&record, kind) {
            continue;
        }
        if !needle.is_empty() && !record.item_code.trim().eq_ignore_ascii_case(needle) {
            continue;
        }
        result.push(record);
    }
    result
}

fn record_matches_supplier_breakdown(record: &DispatchRecord, kind: &str) -> bool {
    match kind.trim() {
        "pending" => record.status == "pending" || record.status == "draft",
        "submitted" => record.status == "accepted",
        "returned" => {
            record.status == "partial"
                || record.status == "rejected"
                || record.status == "cancelled"
        }
        _ => false,
    }
}

fn dispatch_record_needs_comment_scan(record: &DispatchRecord) -> bool {
    matches!(record.status.as_str(), "partial" | "rejected" | "cancelled")
        || !record.note.trim().is_empty()
}

fn is_supplier_acknowledgment_comment(content: &str) -> bool {
    let (author, body) = parse_notification_comment(content);
    author.starts_with("Supplier") && body.trim().to_lowercase().starts_with("tasdiqlayman")
}

fn parse_notification_comment(content: &str) -> (String, String) {
    let trimmed = sanitize_notification_comment(content);
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }
    let lines = trimmed.lines().collect::<Vec<_>>();
    if lines.len() >= 2 {
        let head = lines[0].trim();
        let body = lines[1..].join("\n").trim().to_string();
        if !body.is_empty()
            && ["Supplier", "Werka", "Customer", "Admin"]
                .iter()
                .any(|prefix| head.starts_with(prefix))
        {
            return (head.to_string(), body);
        }
    }
    ("Tizim".to_string(), trimmed)
}

fn sanitize_notification_comment(content: &str) -> String {
    content
        .trim()
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("\r\n", "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
