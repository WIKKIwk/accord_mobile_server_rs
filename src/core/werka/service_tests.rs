use std::sync::Arc;

use async_trait::async_trait;

use time::Date;

use super::models::{
    DispatchRecord, WerkaArchiveResponse, WerkaArchiveSummary, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use super::ports::{WerkaHomeLookup, WerkaPortError};
use super::service::WerkaService;

#[tokio::test]
async fn home_returns_none_without_lookup() {
    let data = WerkaService::new().home(20).await.expect("home result");

    assert!(data.is_none());
}

#[tokio::test]
async fn summary_returns_none_without_lookup() {
    let data = WerkaService::new().summary().await.expect("summary result");

    assert!(data.is_none());
}

#[tokio::test]
async fn pending_returns_none_without_lookup() {
    let data = WerkaService::new()
        .pending(0)
        .await
        .expect("pending result");

    assert!(data.is_none());
}

#[tokio::test]
async fn history_returns_none_without_lookup() {
    let data = WerkaService::new().history().await.expect("history result");

    assert!(data.is_none());
}

#[tokio::test]
async fn status_breakdown_returns_none_without_lookup() {
    let data = WerkaService::new()
        .status_breakdown("pending")
        .await
        .expect("status breakdown result");

    assert!(data.is_none());
}

#[tokio::test]
async fn status_details_returns_none_without_lookup() {
    let data = WerkaService::new()
        .status_details("pending", "SUP-001")
        .await
        .expect("status details result");

    assert!(data.is_none());
}

#[tokio::test]
async fn archive_returns_none_without_lookup() {
    let data = WerkaService::new()
        .archive("sent", "yearly", None, None)
        .await
        .expect("archive result");

    assert!(data.is_none());
}

#[tokio::test]
async fn home_preloads_from_lookup_with_limit() {
    let data = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .home(20)
        .await
        .expect("home result")
        .expect("home data");

    assert_eq!(data.summary.pending_count, 1);
    assert_eq!(data.pending_items[0].id, "PR-001");
}

#[tokio::test]
async fn summary_uses_lookup() {
    let summary = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .summary()
        .await
        .expect("summary result")
        .expect("summary data");

    assert_eq!(summary.pending_count, 1);
    assert_eq!(summary.confirmed_count, 2);
    assert_eq!(summary.returned_count, 3);
}

#[tokio::test]
async fn pending_uses_lookup_with_limit() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .pending(7)
        .await
        .expect("pending result")
        .expect("pending data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "PR-001");
}

#[tokio::test]
async fn history_uses_lookup() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .history()
        .await
        .expect("history result")
        .expect("history data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].event_type, "supplier_ack");
}

#[tokio::test]
async fn status_breakdown_uses_lookup_with_kind() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .status_breakdown("returned")
        .await
        .expect("status breakdown result")
        .expect("status breakdown data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].supplier_ref, "SUP-001");
}

#[tokio::test]
async fn status_details_uses_lookup_with_kind_and_supplier() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .status_details("pending", "SUP-001")
        .await
        .expect("status details result")
        .expect("status details data");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "PR-001");
}

#[tokio::test]
async fn archive_uses_lookup_with_filters() {
    let items = WerkaService::new()
        .with_lookup(Arc::new(FakeWerkaHomeLookup))
        .archive("sent", "monthly", None, None)
        .await
        .expect("archive result")
        .expect("archive data");

    assert_eq!(items.kind, "sent");
    assert_eq!(items.period, "monthly");
    assert_eq!(items.summary.record_count, 1);
}

struct FakeWerkaHomeLookup;

#[async_trait]
impl WerkaHomeLookup for FakeWerkaHomeLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary {
            pending_count: 1,
            confirmed_count: 2,
            returned_count: 3,
        })
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        assert_eq!(pending_limit, 20);
        Ok(WerkaHomeData {
            summary: WerkaHomeSummary {
                pending_count: 1,
                confirmed_count: 2,
                returned_count: 3,
            },
            pending_items: vec![DispatchRecord {
                id: "PR-001".to_string(),
                supplier_name: "Supplier".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Item".to_string(),
                uom: "Kg".to_string(),
                sent_qty: 10.0,
                accepted_qty: 0.0,
                status: "pending".to_string(),
                created_label: "2026-01-16T10:00:00Z".to_string(),
                ..DispatchRecord::default()
            }],
        })
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(limit, 7);
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16T10:00:00Z".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(vec![DispatchRecord {
            id: "supplier_ack:COMM-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 10.0,
            event_type: "supplier_ack".to_string(),
            status: "accepted".to_string(),
            created_label: "2026-01-16T10:00:00Z".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        assert_eq!(kind, "returned");
        Ok(vec![WerkaStatusBreakdownEntry {
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            receipt_count: 1,
            total_sent_qty: 10.0,
            total_accepted_qty: 8.0,
            total_returned_qty: 2.0,
            uom: "Kg".to_string(),
        }])
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(kind, "pending");
        assert_eq!(supplier_ref, "SUP-001");
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16T10:00:00Z".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_archive(
        &self,
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        assert_eq!(kind, "sent");
        assert_eq!(period, "monthly");
        assert!(from.is_none());
        assert!(to.is_none());
        Ok(WerkaArchiveResponse {
            kind: "sent".to_string(),
            period: "monthly".to_string(),
            summary: WerkaArchiveSummary {
                record_count: 1,
                totals_by_uom: Vec::new(),
            },
            items: Vec::new(),
            ..WerkaArchiveResponse::default()
        })
    }
}
