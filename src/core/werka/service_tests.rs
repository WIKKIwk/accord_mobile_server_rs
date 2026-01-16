use std::sync::Arc;

use async_trait::async_trait;

use super::models::{DispatchRecord, WerkaHomeData, WerkaHomeSummary};
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
}
