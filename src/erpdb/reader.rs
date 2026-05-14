use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use sqlx::{MySqlPool, query_as};
use time::Date;

use crate::config::DirectDbConfig;
use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, DispatchRecord, StockEntryBarcodeEntry,
    SupplierDirectoryEntry, SupplierItem, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::erpdb::werka_archive::read_werka_archive;
use crate::erpdb::werka_customers::read_werka_customers;
use crate::erpdb::werka_history::{SupplierAckRow, build_werka_history};
use crate::erpdb::werka_home::{DeliveryNoteSummaryRow, PurchaseReceiptSummaryRow};
use crate::erpdb::werka_items::{
    read_werka_customer_item_options, read_werka_customer_items, read_werka_supplier_items,
};
use crate::erpdb::werka_status_details::build_werka_status_details;
use crate::erpdb::werka_suppliers::read_werka_suppliers;

#[derive(Clone)]
pub struct DirectDbReader {
    pub(crate) pool: MySqlPool,
    pub(crate) encryption_key: String,
    pub(crate) default_warehouse: String,
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
            .min_connections(config.pool.min_connections)
            .max_connections(config.pool.max_connections)
            .acquire_timeout(config.pool.acquire_timeout)
            .idle_timeout(config.pool.idle_timeout)
            .connect_lazy_with(options);

        Self {
            pool,
            encryption_key: config.encryption_key.trim().to_string(),
            default_warehouse: config.default_warehouse.trim().to_string(),
        }
    }

    pub(crate) async fn home(&self, pending_limit: usize) -> Result<WerkaHomeData, sqlx::Error> {
        Ok(WerkaHomeData {
            summary: self.summary().await?,
            pending_items: self.pending(pending_limit).await?,
        })
    }

    pub(crate) async fn summary(&self) -> Result<WerkaHomeSummary, sqlx::Error> {
        let summary = query_as::<_, WerkaSummaryPushdownRow>(WERKA_SUMMARY_PUSHDOWN_SQL)
            .fetch_one(&self.pool)
            .await?;
        Ok(summary.into())
    }

    pub(crate) async fn pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        let limit = clamp_limit(limit, 1000);
        let rows = if limit > 0 {
            query_as::<_, WerkaDispatchRecordPushdownRow>(WERKA_PENDING_PUSHDOWN_LIMIT_SQL)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            query_as::<_, WerkaDispatchRecordPushdownRow>(WERKA_PENDING_PUSHDOWN_SQL)
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn history(&self) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        const RECENT_LIMIT: usize = 120;
        let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool);
        let acks = query_as::<_, SupplierAckRow>(SUPPLIER_ACK_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool);
        let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_LIMIT_SQL)
            .bind(RECENT_LIMIT as i64)
            .fetch_all(&self.pool);
        let (receipts, acks, delivery_notes) = tokio::try_join!(receipts, acks, delivery_notes)?;

        Ok(build_werka_history(
            &receipts,
            &acks,
            &delivery_notes,
            RECENT_LIMIT,
        ))
    }

    pub(crate) async fn status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, sqlx::Error> {
        let kind = match kind.trim() {
            "pending" | "confirmed" | "returned" => kind.trim(),
            _ => return Ok(Vec::new()),
        };
        let rows =
            query_as::<_, WerkaStatusBreakdownPushdownRow>(WERKA_STATUS_BREAKDOWN_PUSHDOWN_SQL)
                .bind(kind)
                .bind(kind)
                .bind(kind)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, sqlx::Error> {
        let supplier_ref = supplier_ref.trim();
        let sql = match kind.trim() {
            "pending" => {
                let receipts = query_as::<_, PurchaseReceiptSummaryRow>(PURCHASE_RECEIPT_ROWS_SQL)
                    .fetch_all(&self.pool);
                let delivery_notes = query_as::<_, DeliveryNoteSummaryRow>(DELIVERY_NOTE_ROWS_SQL)
                    .fetch_all(&self.pool);
                let (receipts, delivery_notes) = tokio::try_join!(receipts, delivery_notes)?;
                return Ok(build_werka_status_details(
                    &receipts,
                    &delivery_notes,
                    kind,
                    supplier_ref,
                ));
            }
            "confirmed" => WERKA_STATUS_DETAILS_CONFIRMED_SQL,
            "returned" => WERKA_STATUS_DETAILS_RETURNED_SQL,
            _ => return Ok(Vec::new()),
        };
        let rows = query_as::<_, WerkaDispatchRecordPushdownRow>(sql)
            .bind(supplier_ref)
            .bind(supplier_ref)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub(crate) async fn archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, sqlx::Error> {
        read_werka_archive(&self.pool, kind, period, from, to).await
    }

    pub(crate) async fn suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, sqlx::Error> {
        read_werka_suppliers(&self.pool, query, limit, offset).await
    }

    pub(crate) async fn customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, sqlx::Error> {
        read_werka_customers(&self.pool, query, limit, offset).await
    }

    pub(crate) async fn supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, sqlx::Error> {
        read_werka_supplier_items(
            &self.pool,
            &self.default_warehouse,
            supplier_ref,
            query,
            limit,
            offset,
        )
        .await
    }

    pub(crate) async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, sqlx::Error> {
        read_werka_customer_items(
            &self.pool,
            &self.default_warehouse,
            customer_ref,
            query,
            limit,
            offset,
        )
        .await
    }

    pub(crate) async fn customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, sqlx::Error> {
        read_werka_customer_item_options(&self.pool, &self.default_warehouse, query, limit, offset)
            .await
    }

    pub(crate) async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, sqlx::Error> {
        crate::erpdb::stock_entry::read_stock_entries_by_barcode(&self.pool, barcode, limit).await
    }
}

#[derive(Debug, sqlx::FromRow)]
struct WerkaSummaryPushdownRow {
    pending_count: i64,
    confirmed_count: i64,
    returned_count: i64,
}

impl From<WerkaSummaryPushdownRow> for WerkaHomeSummary {
    fn from(row: WerkaSummaryPushdownRow) -> Self {
        Self {
            pending_count: row.pending_count,
            confirmed_count: row.confirmed_count,
            returned_count: row.returned_count,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct WerkaDispatchRecordPushdownRow {
    id: String,
    record_type: String,
    supplier_ref: String,
    supplier_name: String,
    item_code: String,
    item_name: String,
    uom: String,
    sent_qty: f64,
    accepted_qty: f64,
    amount: f64,
    currency: String,
    status: String,
    created_label: String,
}

impl From<WerkaDispatchRecordPushdownRow> for DispatchRecord {
    fn from(row: WerkaDispatchRecordPushdownRow) -> Self {
        Self {
            id: row.id,
            record_type: row.record_type,
            supplier_ref: row.supplier_ref,
            supplier_name: row.supplier_name,
            item_code: row.item_code,
            item_name: row.item_name,
            uom: row.uom,
            sent_qty: row.sent_qty,
            accepted_qty: row.accepted_qty,
            amount: row.amount,
            currency: row.currency,
            status: row.status,
            created_label: row.created_label,
            ..Self::default()
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct WerkaStatusBreakdownPushdownRow {
    supplier_ref: String,
    supplier_name: String,
    receipt_count: i64,
    total_sent_qty: f64,
    total_accepted_qty: f64,
    total_returned_qty: f64,
    uom: String,
}

impl From<WerkaStatusBreakdownPushdownRow> for WerkaStatusBreakdownEntry {
    fn from(row: WerkaStatusBreakdownPushdownRow) -> Self {
        Self {
            supplier_ref: row.supplier_ref,
            supplier_name: row.supplier_name,
            receipt_count: row.receipt_count,
            total_sent_qty: row.total_sent_qty,
            total_accepted_qty: row.total_accepted_qty,
            total_returned_qty: row.total_returned_qty,
            uom: row.uom,
        }
    }
}

const PURCHASE_RECEIPT_ROWS_SQL: &str = r#"
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
    ORDER BY pr.name DESC
"#;

const DELIVERY_NOTE_ROWS_SQL: &str = r#"
    SELECT
        dn.name AS name,
        dn.customer AS customer,
        COALESCE(dn.customer_name, '') AS customer_name,
        dn.docstatus AS doc_status,
        COALESCE(CAST(dn.modified AS CHAR), '') AS modified,
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
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
        CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS qty,
        CAST(COALESCE(dni.returned_qty, 0) AS DOUBLE) AS returned_qty,
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

const WERKA_SUMMARY_PUSHDOWN_SQL: &str = r#"
    SELECT
        CAST(COALESCE(SUM(status IN ('pending', 'draft')), 0) AS SIGNED) AS pending_count,
        CAST(COALESCE(SUM(status = 'accepted'), 0) AS SIGNED) AS confirmed_count,
        CAST(COALESCE(SUM(status IN ('partial', 'rejected', 'cancelled')), 0) AS SIGNED) AS returned_count
    FROM (
        SELECT
            CASE
                WHEN pr.docstatus = 2 OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled' THEN 'cancelled'
                WHEN pr.docstatus = 1 THEN
                    CASE
                        WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                        WHEN GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) > 0
                        AND COALESCE(pr.total_qty, 0) < GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) THEN 'partial'
                        ELSE 'accepted'
                    END
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status
        FROM `tabPurchase Receipt` pr
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND NOT (pr.docstatus = 0 AND COALESCE(pr.remarks, '') LIKE '%Accord Werka Aytilmagan: pending%')
          AND NOT (pr.docstatus = 1 AND COALESCE(pr.remarks, '') LIKE '%Accord Werka Aytilmagan: approved%')
        UNION ALL
        SELECT
            CASE COALESCE(dn.accord_customer_state, 0)
                WHEN 2 THEN 'rejected'
                WHEN 3 THEN 'accepted'
                WHEN 4 THEN 'partial'
                ELSE 'pending'
            END AS status
        FROM `tabDelivery Note` dn
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
    ) statuses
"#;

const WERKA_STATUS_BREAKDOWN_PUSHDOWN_SQL: &str = r#"
    WITH records AS (
        SELECT
            0 AS source_order,
            pr.name AS sort_name,
            pr.supplier AS supplier_ref,
            COALESCE(pr.supplier_name, '') AS supplier_name,
            COALESCE(pri.uom, '') AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CASE
                WHEN pr.docstatus = 1 AND COALESCE(pr.total_qty, 0) > 0
                THEN CAST(COALESCE(pr.total_qty, 0) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CASE
                WHEN pr.docstatus = 2 OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled' THEN 'cancelled'
                WHEN pr.docstatus = 1 THEN
                    CASE
                        WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                        WHEN GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) > 0
                        AND COALESCE(pr.total_qty, 0) < GREATEST(
                            COALESCE(pr.total_qty, 0),
                            CASE
                                WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                     REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                                THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                                ELSE COALESCE(pr.total_qty, 0)
                            END
                        ) THEN 'partial'
                        ELSE 'accepted'
                    END
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
        UNION ALL
        SELECT
            1 AS source_order,
            dn.name AS sort_name,
            dn.customer AS supplier_ref,
            COALESCE(dn.customer_name, '') AS supplier_name,
            COALESCE(dni.uom, '') AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CASE
                WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN CAST(COALESCE(dn.total_qty, 0) AS DOUBLE)
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN CAST(GREATEST(
                    COALESCE(dn.total_qty, 0) -
                    CASE
                        WHEN COALESCE(dni.returned_qty, 0) <= 0 THEN COALESCE(dn.total_qty, 0)
                        ELSE COALESCE(dni.returned_qty, 0)
                    END,
                    0
                ) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CASE
                WHEN dn.docstatus != 1 THEN 'draft'
                WHEN COALESCE(dn.accord_flow_state, 0) != 1 THEN 'pending'
                WHEN COALESCE(dn.accord_customer_state, 0) = 2 THEN 'rejected'
                WHEN COALESCE(dn.accord_customer_state, 0) = 3 THEN 'accepted'
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                ELSE 'pending'
            END AS status
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
    ),
    matching_records AS (
        SELECT
            *,
            CASE
                WHEN TRIM(COALESCE(supplier_ref, '')) = '' THEN TRIM(COALESCE(supplier_name, ''))
                ELSE TRIM(COALESCE(supplier_ref, ''))
            END AS group_key
        FROM records
        WHERE
            (? = 'pending' AND status IN ('pending', 'draft'))
            OR (? = 'confirmed' AND status = 'accepted')
            OR (? = 'returned' AND status IN ('partial', 'rejected', 'cancelled'))
    ),
    ranked_records AS (
        SELECT
            *,
            ROW_NUMBER() OVER (PARTITION BY group_key ORDER BY source_order ASC, sort_name DESC) AS group_row_number
        FROM matching_records
    )
    SELECT
        COALESCE(MAX(CASE WHEN group_row_number = 1 THEN supplier_ref END), '') AS supplier_ref,
        COALESCE(MAX(CASE WHEN group_row_number = 1 THEN supplier_name END), '') AS supplier_name,
        CAST(COUNT(*) AS SIGNED) AS receipt_count,
        CAST(COALESCE(SUM(sent_qty), 0) AS DOUBLE) AS total_sent_qty,
        CAST(COALESCE(SUM(accepted_qty), 0) AS DOUBLE) AS total_accepted_qty,
        CAST(COALESCE(SUM(GREATEST(sent_qty - accepted_qty, 0)), 0) AS DOUBLE) AS total_returned_qty,
        COALESCE(MAX(CASE WHEN group_row_number = 1 THEN uom END), '') AS uom
    FROM ranked_records
    GROUP BY group_key
    ORDER BY receipt_count DESC, LOWER(supplier_name) ASC
"#;

const WERKA_STATUS_DETAILS_CONFIRMED_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            TRIM(COALESCE(pr.name, '')) AS id,
            'purchase_receipt' AS record_type,
            TRIM(COALESCE(pr.supplier, '')) AS supplier_ref,
            TRIM(COALESCE(pr.supplier_name, '')) AS supplier_name,
            TRIM(COALESCE(pri.item_code, '')) AS item_code,
            TRIM(COALESCE(pri.item_name, '')) AS item_name,
            TRIM(COALESCE(pri.uom, '')) AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            'accepted' AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 1
          AND LOWER(TRIM(COALESCE(pr.status, ''))) != 'cancelled'
          AND COALESCE(pr.total_qty, 0) > 0
          AND COALESCE(pr.total_qty, 0) >= GREATEST(
              COALESCE(pr.total_qty, 0),
              CASE
                  WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                       REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                  THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                  ELSE COALESCE(pr.total_qty, 0)
              END
          )
        UNION ALL
        SELECT
            TRIM(COALESCE(dn.name, '')) AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            'accepted' AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state = 3
    ) records
    WHERE (? = '' OR LOWER(TRIM(supplier_ref)) = LOWER(TRIM(?)))
    ORDER BY created_label DESC
"#;

const WERKA_STATUS_DETAILS_RETURNED_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            TRIM(COALESCE(pr.name, '')) AS id,
            'purchase_receipt' AS record_type,
            TRIM(COALESCE(pr.supplier, '')) AS supplier_ref,
            TRIM(COALESCE(pr.supplier_name, '')) AS supplier_name,
            TRIM(COALESCE(pri.item_code, '')) AS item_code,
            TRIM(COALESCE(pri.item_name, '')) AS item_name,
            TRIM(COALESCE(pri.uom, '')) AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CASE
                WHEN pr.docstatus = 1
                 AND LOWER(TRIM(COALESCE(pr.status, ''))) != 'cancelled'
                 AND COALESCE(pr.total_qty, 0) > 0
                 AND COALESCE(pr.total_qty, 0) < GREATEST(
                    COALESCE(pr.total_qty, 0),
                    CASE
                        WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                             REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                        THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                        ELSE COALESCE(pr.total_qty, 0)
                    END
                 )
                THEN CAST(COALESCE(pr.total_qty, 0) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            CASE
                WHEN pr.docstatus = 2 OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled' THEN 'cancelled'
                WHEN COALESCE(pr.total_qty, 0) <= 0 THEN 'rejected'
                ELSE 'partial'
            END AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND (
              pr.docstatus = 2
              OR LOWER(TRIM(COALESCE(pr.status, ''))) = 'cancelled'
              OR (
                  pr.docstatus = 1
                  AND (
                      COALESCE(pr.total_qty, 0) <= 0
                      OR COALESCE(pr.total_qty, 0) < GREATEST(
                          COALESCE(pr.total_qty, 0),
                          CASE
                              WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                                   REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                              THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                              ELSE COALESCE(pr.total_qty, 0)
                          END
                      )
                  )
              )
          )
        UNION ALL
        SELECT
            TRIM(COALESCE(dn.name, '')) AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CASE
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN CAST(GREATEST(
                    COALESCE(dn.total_qty, 0) -
                    CASE
                        WHEN COALESCE(dni.returned_qty, 0) <= 0 THEN GREATEST(COALESCE(dn.total_qty, 0), 0)
                        ELSE COALESCE(dni.returned_qty, 0)
                    END,
                    0
                ) AS DOUBLE)
                ELSE CAST(0 AS DOUBLE)
            END AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            CASE
                WHEN COALESCE(dn.accord_customer_state, 0) = 4 THEN 'partial'
                ELSE 'rejected'
            END AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state IN (2, 4)
    )
    records
    WHERE (? = '' OR LOWER(TRIM(supplier_ref)) = LOWER(TRIM(?)))
    ORDER BY created_label DESC
"#;

const SUPPLIER_ACK_ROWS_LIMIT_SQL: &str = r#"
    SELECT
        c.name AS comment_id,
        COALESCE(CAST(c.creation AS CHAR), '') AS created_label,
        pr.supplier AS supplier_ref,
        COALESCE(pr.supplier_name, '') AS supplier_name,
        CAST(COALESCE(pr.total_qty, 0) AS DOUBLE) AS sent_qty,
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

const WERKA_PENDING_PUSHDOWN_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            pr.name AS id,
            'purchase_receipt' AS record_type,
            TRIM(COALESCE(pr.supplier, '')) AS supplier_ref,
            TRIM(COALESCE(pr.supplier_name, '')) AS supplier_name,
            TRIM(COALESCE(pri.item_code, '')) AS item_code,
            TRIM(COALESCE(pri.item_name, '')) AS item_name,
            TRIM(COALESCE(pri.uom, '')) AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            CASE
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 0
          AND COALESCE(pr.remarks, '') NOT LIKE '%Accord Werka Aytilmagan: pending%'
        UNION ALL
        SELECT
            dn.name AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            'pending' AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state NOT IN (2, 3, 4)
    ) pending_rows
    ORDER BY created_label DESC
"#;

const WERKA_PENDING_PUSHDOWN_LIMIT_SQL: &str = r#"
    SELECT *
    FROM (
        SELECT
            pr.name AS id,
            'purchase_receipt' AS record_type,
            TRIM(COALESCE(pr.supplier, '')) AS supplier_ref,
            TRIM(COALESCE(pr.supplier_name, '')) AS supplier_name,
            TRIM(COALESCE(pri.item_code, '')) AS item_code,
            TRIM(COALESCE(pri.item_name, '')) AS item_name,
            TRIM(COALESCE(pri.uom, '')) AS uom,
            CAST(GREATEST(
                COALESCE(pr.total_qty, 0),
                CASE
                    WHEN SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1)
                         REGEXP '^-?[0-9]+(\\.[0-9]+)?$'
                    THEN CAST(SUBSTRING_INDEX(COALESCE(pr.supplier_delivery_note, ''), ':', -1) AS DECIMAL(18,4))
                    ELSE COALESCE(pr.total_qty, 0)
                END
            ) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(COALESCE(pri.amount, 0) AS DOUBLE) AS amount,
            TRIM(COALESCE(pr.currency, '')) AS currency,
            CASE
                WHEN LOWER(TRIM(COALESCE(pr.status, ''))) = 'draft' THEN 'draft'
                ELSE 'pending'
            END AS status,
            TRIM(COALESCE(CAST(pr.posting_date AS CHAR), '')) AS created_label
        FROM `tabPurchase Receipt` pr
        LEFT JOIN `tabPurchase Receipt Item` pri ON pri.parent = pr.name AND pri.idx = 1
        WHERE pr.supplier_delivery_note LIKE 'TG:%'
          AND pr.docstatus = 0
          AND COALESCE(pr.remarks, '') NOT LIKE '%Accord Werka Aytilmagan: pending%'
        UNION ALL
        SELECT
            dn.name AS id,
            'delivery_note' AS record_type,
            TRIM(COALESCE(dn.customer, '')) AS supplier_ref,
            TRIM(COALESCE(dn.customer_name, '')) AS supplier_name,
            TRIM(COALESCE(dni.item_code, '')) AS item_code,
            TRIM(COALESCE(dni.item_name, '')) AS item_name,
            TRIM(COALESCE(dni.uom, '')) AS uom,
            CAST(COALESCE(dn.total_qty, 0) AS DOUBLE) AS sent_qty,
            CAST(0 AS DOUBLE) AS accepted_qty,
            CAST(0 AS DOUBLE) AS amount,
            '' AS currency,
            'pending' AS status,
            TRIM(COALESCE(CAST(dn.modified AS CHAR), '')) AS created_label
        FROM `tabDelivery Note` dn
        LEFT JOIN `tabDelivery Note Item` dni ON dni.parent = dn.name AND dni.idx = 1
        WHERE dn.docstatus = 1
          AND dn.accord_flow_state = 1
          AND dn.accord_customer_state NOT IN (2, 3, 4)
    ) pending_rows
    ORDER BY created_label DESC
    LIMIT ?
"#;

fn clamp_limit(limit: usize, max: usize) -> usize {
    if limit > max { max } else { limit }
}
