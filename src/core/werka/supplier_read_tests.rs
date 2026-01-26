use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::ports::{
    PurchaseReceiptComment, PurchaseReceiptDraft, SupplierPurchaseReceiptLookup, WerkaPortError,
};
use super::service::WerkaService;

#[tokio::test]
async fn supplier_summary_counts_erp_receipt_statuses_like_go_fallback() {
    let service = WerkaService::new().with_supplier_purchase_receipt_lookup(std::sync::Arc::new(
        FakeSupplierReceipts {
            calls: Mutex::new(Vec::new()),
            comments_calls: Mutex::new(Vec::new()),
            receipts: vec![
                receipt(
                    "PR-PENDING",
                    0,
                    "To Bill",
                    5.0,
                    "TG:+998:20260126090000:5.0000",
                ),
                receipt("PR-DRAFT", 0, "Draft", 5.0, "TG:+998:20260126090001:5.0000"),
                receipt(
                    "PR-OK",
                    1,
                    "Completed",
                    5.0,
                    "TG:+998:20260126090002:5.0000",
                ),
                receipt(
                    "PR-PARTIAL",
                    1,
                    "Completed",
                    3.0,
                    "TG:+998:20260126090003:5.0000",
                ),
                receipt(
                    "PR-CANCELLED",
                    2,
                    "Cancelled",
                    5.0,
                    "TG:+998:20260126090004:5.0000",
                ),
            ],
        },
    ));

    let summary = service
        .supplier_summary("SUP-001", "Supplier")
        .await
        .expect("summary result")
        .expect("summary");

    assert_eq!(summary.pending_count, 2);
    assert_eq!(summary.submitted_count, 1);
    assert_eq!(summary.returned_count, 2);
}

#[tokio::test]
async fn supplier_history_adds_supplier_ack_note_only_for_records_that_need_scan() {
    let service = WerkaService::new().with_supplier_purchase_receipt_lookup(std::sync::Arc::new(
        FakeSupplierReceipts {
            calls: Mutex::new(Vec::new()),
            comments_calls: Mutex::new(Vec::new()),
            receipts: vec![
                receipt(
                    "PR-CLEAN",
                    0,
                    "To Bill",
                    5.0,
                    "TG:+998:20260126090000:5.0000",
                ),
                receipt(
                    "PR-PARTIAL",
                    1,
                    "Completed",
                    3.0,
                    "TG:+998:20260126090001:5.0000",
                ),
            ],
        },
    ));

    let items = service
        .supplier_history("SUP-001", "Supplier")
        .await
        .expect("history result")
        .expect("history");

    assert_eq!(items.len(), 2);
    assert!(items[0].note.is_empty());
    assert_eq!(
        items[1].note,
        "Supplier tasdiqladi: Tasdiqlayman, shu holat bo‘lganini ko‘rdim."
    );
}

struct FakeSupplierReceipts {
    calls: Mutex<Vec<(usize, usize)>>,
    comments_calls: Mutex<Vec<Vec<String>>>,
    receipts: Vec<PurchaseReceiptDraft>,
}

#[async_trait]
impl SupplierPurchaseReceiptLookup for FakeSupplierReceipts {
    async fn list_supplier_purchase_receipts_page(
        &self,
        _supplier_ref: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<PurchaseReceiptDraft>, WerkaPortError> {
        self.calls.lock().expect("calls").push((limit, offset));
        Ok(self.receipts.clone())
    }

    async fn list_supplier_purchase_receipt_comments_batch(
        &self,
        names: &[String],
        _limit: usize,
    ) -> Result<HashMap<String, Vec<PurchaseReceiptComment>>, WerkaPortError> {
        self.comments_calls
            .lock()
            .expect("comments calls")
            .push(names.to_vec());
        let mut result = HashMap::new();
        for name in names {
            result.insert(
                name.clone(),
                vec![PurchaseReceiptComment {
                    id: "COMM-001".to_string(),
                    content: "Supplier • Supplier\nTasdiqlayman, shu holat bo‘lganini ko‘rdim."
                        .to_string(),
                    created_at: "2026-01-26 09:00:00".to_string(),
                }],
            );
        }
        Ok(result)
    }
}

fn receipt(
    name: &str,
    doc_status: i32,
    status: &str,
    qty: f64,
    marker: &str,
) -> PurchaseReceiptDraft {
    PurchaseReceiptDraft {
        name: name.to_string(),
        doc_status,
        status: status.to_string(),
        supplier: "SUP-001".to_string(),
        supplier_name: "Supplier".to_string(),
        posting_date: "2026-01-26".to_string(),
        supplier_delivery_note: marker.to_string(),
        item_code: "ITEM-001".to_string(),
        item_name: "Item".to_string(),
        qty,
        uom: "Nos".to_string(),
        ..PurchaseReceiptDraft::default()
    }
}
