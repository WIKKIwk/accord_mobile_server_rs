use async_trait::async_trait;
use sqlx::query_as;

use crate::core::werka::models::{DispatchRecord, SupplierHomeSummary};
use crate::core::werka::ports::{SupplierReadLookup, WerkaPortError};
use crate::erpdb::reader::DirectDbReader;
use crate::erpdb::werka_home::{
    PurchaseReceiptSummaryRow, classify_werka_receipt, purchase_receipt_to_record,
};
use crate::erpdb::werka_lookup::database_error;

#[async_trait]
impl SupplierReadLookup for DirectDbReader {
    async fn supplier_summary(
        &self,
        supplier_ref: &str,
    ) -> Result<SupplierHomeSummary, WerkaPortError> {
        let rows = query_as::<_, PurchaseReceiptSummaryRow>(SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL)
            .bind(supplier_ref.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(build_supplier_summary(&rows))
    }

    async fn supplier_history(
        &self,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        let rows = query_as::<_, PurchaseReceiptSummaryRow>(SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL)
            .bind(supplier_ref.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(build_supplier_history(&rows))
    }
}

fn build_supplier_summary(rows: &[PurchaseReceiptSummaryRow]) -> SupplierHomeSummary {
    let mut summary = SupplierHomeSummary::default();
    for row in rows {
        let (status, _) = classify_werka_receipt(row);
        match status.as_str() {
            "pending" | "draft" => summary.pending_count += 1,
            "accepted" => summary.submitted_count += 1,
            "partial" | "rejected" | "cancelled" => summary.returned_count += 1,
            _ => {}
        }
    }
    summary
}

fn build_supplier_history(rows: &[PurchaseReceiptSummaryRow]) -> Vec<DispatchRecord> {
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let (_, include) = classify_werka_receipt(row);
        if include {
            result.push(purchase_receipt_to_record(row));
        }
    }
    result.sort_by(|left, right| right.created_label.cmp(&left.created_label));
    result
}

const SUPPLIER_PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
      AND pr.supplier = ?
    ORDER BY pr.name DESC
"#;

#[cfg(test)]
mod tests {
    use super::{build_supplier_history, build_supplier_summary};
    use crate::erpdb::werka_home::PurchaseReceiptSummaryRow;

    #[test]
    fn supplier_summary_counts_statuses_like_go_reader() {
        let rows = vec![
            receipt("PR-PENDING", 0, "To Bill", 5.0, ""),
            receipt("PR-DRAFT", 0, "Draft", 5.0, ""),
            receipt("PR-OK", 1, "Completed", 5.0, ""),
            receipt(
                "PR-PARTIAL",
                1,
                "Completed",
                3.0,
                "TG:+998:20260126090003:5.0000",
            ),
            receipt("PR-CANCELLED", 2, "Cancelled", 5.0, ""),
        ];

        let summary = build_supplier_summary(&rows);

        assert_eq!(summary.pending_count, 2);
        assert_eq!(summary.submitted_count, 1);
        assert_eq!(summary.returned_count, 2);
    }

    #[test]
    fn supplier_history_filters_hidden_unannounced_and_sorts_like_go_reader() {
        let mut pending = receipt("PR-PENDING", 0, "To Bill", 5.0, "");
        pending.posting_date = "2026-01-26".to_string();
        let mut hidden = receipt("PR-HIDDEN", 0, "To Bill", 5.0, "");
        hidden.posting_date = "2026-01-27".to_string();
        hidden.remarks = "Accord Werka Aytilmagan: pending".to_string();
        let mut accepted = receipt("PR-ACCEPTED", 1, "Completed", 5.0, "");
        accepted.posting_date = "2026-01-28".to_string();

        let items = build_supplier_history(&[pending, hidden, accepted]);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "PR-ACCEPTED");
        assert_eq!(items[1].id, "PR-PENDING");
    }

    fn receipt(
        name: &str,
        doc_status: i32,
        status: &str,
        qty: f64,
        marker: &str,
    ) -> PurchaseReceiptSummaryRow {
        PurchaseReceiptSummaryRow {
            name: name.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            doc_status,
            status: status.to_string(),
            total_qty: qty,
            posting_date: "2026-01-26".to_string(),
            supplier_delivery_note: if marker.is_empty() {
                "TG:+998:20260126090000:5.0000".to_string()
            } else {
                marker.to_string()
            },
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Nos".to_string(),
            ..PurchaseReceiptSummaryRow::default()
        }
    }
}
