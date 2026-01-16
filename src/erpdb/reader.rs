use async_trait::async_trait;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::{MySqlPool, query_as};
use time::Date;

use crate::config::DirectDbConfig;
use crate::core::werka::models::{
    DispatchRecord, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::erpdb::werka_archive::read_werka_archive;
use crate::erpdb::werka_history::{SupplierAckRow, build_werka_history};
use crate::erpdb::werka_home::{
    DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow, build_werka_home,
};
use crate::erpdb::werka_pending::build_werka_pending;
use crate::erpdb::werka_status_breakdown::build_werka_status_breakdown;
use crate::erpdb::werka_status_details::build_werka_status_details;
use crate::erpdb::werka_summary::{
    DeliveryNoteStatusRow, PurchaseReceiptStatusRow, build_werka_summary,
};

#[derive(Clone)]
pub struct DirectDbReader {
    pool: MySqlPool,
}

impl DirectDbReader {
    pub fn new(config: DirectDbConfig) -> Self {
        let options = MySqlConnectOptions::new()
            .host(&config.host)
            .port(config.port)
            .username(&config.user)
            .password(&config.password)
            .database(&config.name);
        let pool = MySqlPoolOptions::new()
            .max_connections(12)
            .connect_lazy_with(options);

        Self { pool }
    }

    async fn home(&self, pending_limit: usize) -> Result<WerkaHomeData, sqlx::Error> {
        let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;
        let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;

        Ok(build_werka_home(&receipts, &delivery_notes, pending_limit))
    }

    async fn summary(&self) -> Result<WerkaHomeSummary, sqlx::Error> {
        let receipts = query_as::<_, PurchaseReceiptStatusRow>(PURCHASE_RECEIPT_STATUS_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;
        let delivery_notes = query_as::<_, DeliveryNoteStatusRow>(DELIVERY_NOTE_STATUS_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;

        Ok(build_werka_summary(&receipts, &delivery_notes))
    }

    async fn pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        let limit = clamp_limit(limit, 1000);
        let receipts = if limit > 0 {
            query_as::<_, PurchaseReceiptSummaryRow>(PENDING_PURCHASE_RECEIPT_ROWS_LIMIT_SQL)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            query_as::<_, PurchaseReceiptSummaryRow>(PENDING_PURCHASE_RECEIPT_ROWS_SQL)
                .fetch_all(&self.pool)
                .await?
        };
        let mut delivery_query = query_as::<_, DeliveryNoteSummaryRow>(if limit > 0 {
            PENDING_DELIVERY_NOTE_ROWS_LIMIT_SQL
        } else {
            PENDING_DELIVERY_NOTE_ROWS_SQL
        })
        .bind(1_i32)
        .bind(2_i32)
        .bind(3_i32)
        .bind(4_i32);
        if limit > 0 {
            delivery_query = delivery_query.bind(limit as i64);
        }
        let delivery_notes = delivery_query.fetch_all(&self.pool).await?;

        Ok(build_werka_pending(&receipts, &delivery_notes, limit))
    }

    async fn history(&self) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        const RECENT_LIMIT: usize = 120;
        let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool)
            .await?;
        let acks = query_as::<_, SupplierAckRow>(SUPPLIER_ACK_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool)
            .await?;
        let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool)
            .await?;

        Ok(build_werka_history(
            &receipts,
            &acks,
            &delivery_notes,
            RECENT_LIMIT,
        ))
    }

    async fn status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, sqlx::Error> {
        let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;
        let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;

        Ok(build_werka_status_breakdown(
            &receipts,
            &delivery_notes,
            kind,
        ))
    }

    async fn status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;
        let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_SQL)
            .fetch_all(&self.pool)
            .await?;

        Ok(build_werka_status_details(
            &receipts,
            &delivery_notes,
            kind,
            supplier_ref,
        ))
    }

    async fn archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, sqlx::Error> {
        read_werka_archive(&self.pool, kind, period, from, to).await
    }
}

#[async_trait]
impl WerkaHomeLookup for DirectDbReader {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        self.summary()
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        self.home(pending_limit)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.pending(limit)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.history()
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        self.status_breakdown(kind)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        self.status_details(kind, supplier_ref)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        self.archive(kind, period, from, to)
            .await
            .map_err(|error| WerkaPortError::Database(error.to_string()))
    }
}

const PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        COALESCE(pr.total_qty, 0) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        COALESCE(pri.amount, 0) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
    ORDER BY pr.name DESC
"#;

const DELIVERY_NOTE_ROWS_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        COALESCE(dn.total_qty, 0) AS qty,
        COALESCE(dni.returned_qty, 0) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ORDER BY dn.name DESC
"#;

const PURCHASE_RECEIPT_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        COALESCE(pr.total_qty, 0) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        COALESCE(pri.amount, 0) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
    ORDER BY pr.name DESC
    LIMIT ?
"#;

const DELIVERY_NOTE_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        COALESCE(dn.total_qty, 0) AS qty,
        COALESCE(dni.returned_qty, 0) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ORDER BY dn.name DESC
    LIMIT ?
"#;

const PURCHASE_RECEIPT_STATUS_ROWS_SQL: &str = r#"
    SELECT
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        COALESCE(pr.total_qty, 0) AS total_qty,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks
    FROM `tabPurchase Receipt` pr
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
"#;

const DELIVERY_NOTE_STATUS_ROWS_SQL: &str = r#"
    SELECT
        dn.docstatus AS doc_status,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
"#;

const SUPPLIER_ACK_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        c.name AS comment_id,
        COALESCE(CAST(c.creation AS CHAR), '') AS created_label,
        pr.supplier AS supplier_ref,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        COALESCE(pr.total_qty, 0) AS sent_qty,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom
    FROM `tabComment` c
    INNER JOIN `tabPurchase Receipt` pr ON pr.name = c.reference_name
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE c.reference_doctype = 'Purchase Receipt'
      AND c.content LIKE 'Supplier%'
      AND c.content LIKE '%Tasdiqlayman%'
    ORDER BY c.name DESC
    LIMIT ?
"#;

const PENDING_PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        COALESCE(pr.total_qty, 0) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        COALESCE(pri.amount, 0) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
      AND pr.docstatus = 0
    ORDER BY pr.name DESC
"#;

const PENDING_PURCHASE_RECEIPT_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        pr.name AS name,
        pr.supplier AS supplier,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        pr.docstatus AS doc_status,
        COALESCE(pr.status, '') AS status,
        COALESCE(pr.total_qty, 0) AS total_qty,
        COALESCE(CAST(pr.posting_date AS CHAR), '') AS posting_date,
        COALESCE(pr.supplier_delivery_note, '') AS supplier_delivery_note,
        COALESCE(pr.remarks, '') AS remarks,
        COALESCE(pr.currency, '') AS currency,
        COALESCE(pri.item_code, '') AS item_code,
        COALESCE(pri.item_name, '') AS item_name,
        COALESCE(pri.uom, '') AS uom,
        COALESCE(pri.amount, 0) AS amount
    FROM `tabPurchase Receipt` pr
    LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
    WHERE pr.supplier_delivery_note LIKE 'TG:%'
      AND pr.docstatus = 0
    ORDER BY pr.name DESC
    LIMIT ?
"#;

const PENDING_DELIVERY_NOTE_ROWS_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        COALESCE(dn.total_qty, 0) AS qty,
        COALESCE(dni.returned_qty, 0) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND COALESCE(dn.accord_flow_state, 0) = ?
      AND COALESCE(dn.accord_customer_state, 0) NOT IN (?, ?, ?)
    ORDER BY dn.name DESC
"#;

const PENDING_DELIVERY_NOTE_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        COALESCE(dn.total_qty, 0) AS qty,
        COALESCE(dni.returned_qty, 0) AS returned_qty,
        COALESCE(dn.accord_customer_reason, '') AS customer_reason,
        COALESCE(dni.item_code, '') AS item_code,
        COALESCE(dni.item_name, '') AS item_name,
        COALESCE(dni.uom, '') AS uom,
        COALESCE(dn.accord_flow_state, 0) AS accord_flow_state,
        COALESCE(dn.accord_customer_state, 0) AS accord_customer_state
    FROM `tabDelivery Note` dn
    LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    WHERE dn.docstatus = 1
      AND COALESCE(dn.accord_flow_state, 0) = ?
      AND COALESCE(dn.accord_customer_state, 0) NOT IN (?, ?, ?)
    ORDER BY dn.name DESC
    LIMIT ?
"#;

fn clamp_limit(limit: usize, max: usize) -> usize {
    if limit > max { max } else { limit }
}
