use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::ports::{
    CreateDeliveryNoteInput, DeliveryNoteStateUpdate, ErpItem, WerkaCustomerIssueWriter,
    WerkaPortError,
};
use crate::core::werka::service::WerkaService;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        erp_url: String::new(),
        erp_api_key: String::new(),
        erp_api_secret: String::new(),
        default_target_warehouse: String::new(),
        erp_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        admin_supplier_store_path: "data/mobile_admin_suppliers.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
        direct_read_enabled: false,
        direct_site_config_path: String::new(),
        direct_db_host: String::new(),
        direct_db_port: None,
        direct_db_user: String::new(),
        direct_db_password: String::new(),
        direct_db_name: String::new(),
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn customer_issue_create_requires_auth() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/customer-issue/create")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn customer_issue_create_rejects_non_post_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/werka/customer-issue/create")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn customer_issue_create_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/customer-issue/create")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from("{"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn customer_issue_create_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "werka customer issue create failed"
    );
}

#[tokio::test]
async fn customer_issue_create_returns_record_and_source_metadata() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::ok()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["entry_id"], "DN-001");
    assert_eq!(value["customer_ref"], "CUST-001");
    assert_eq!(value["item_code"], "ITEM-001");
    assert_eq!(value["uom"], "Kg");
    assert_eq!(value["qty"], 2.0);
}

#[tokio::test]
async fn customer_issue_create_rejects_duplicate_source() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_customer_issue_writer(Arc::new(FakeIssueWriter::duplicate()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let value = json_body(response).await;
    assert_eq!(value["error"], "duplicate customer issue source");
    assert_eq!(value["error_code"], "duplicate_customer_issue_source");
}

#[tokio::test]
async fn customer_issue_create_maps_negative_stock_to_conflict() {
    let mut state = test_state();
    state.werka = WerkaService::new()
        .with_customer_issue_writer(Arc::new(FakeIssueWriter::insufficient_stock()));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let value = json_body(response).await;
    assert_eq!(value["error"], "insufficient stock");
    assert_eq!(value["error_code"], "insufficient_stock");
}

fn request_body() -> &'static str {
    r#"{"customer_ref":"CUST-001","item_code":"ITEM-001","qty":2,"source_barcode":"30AD3353F0C879E4801AD4DF","source_stock_entry":"MAT-STE-2026-00572","source_line_index":1}"#
}

fn create_request(token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/werka/customer-issue/create")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

async fn werka_session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka".to_string(),
            phone: "+99888862440".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

#[derive(Clone, Copy)]
enum FakeIssueMode {
    Ok,
    Duplicate,
    InsufficientStock,
}

struct FakeIssueWriter {
    mode: FakeIssueMode,
}

impl FakeIssueWriter {
    fn ok() -> Self {
        Self {
            mode: FakeIssueMode::Ok,
        }
    }

    fn duplicate() -> Self {
        Self {
            mode: FakeIssueMode::Duplicate,
        }
    }

    fn insufficient_stock() -> Self {
        Self {
            mode: FakeIssueMode::InsufficientStock,
        }
    }
}

#[async_trait]
impl WerkaCustomerIssueWriter for FakeIssueWriter {
    async fn get_items_by_codes(&self, codes: &[String]) -> Result<Vec<ErpItem>, WerkaPortError> {
        assert_eq!(codes, &["ITEM-001".to_string()]);
        Ok(vec![ErpItem {
            code: "ITEM-001".to_string(),
            name: "Item 001".to_string(),
            uom: "Kg".to_string(),
        }])
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        Ok("Stores - A".to_string())
    }

    async fn resolve_company(&self) -> Result<String, WerkaPortError> {
        Ok("Accord".to_string())
    }

    async fn customer_issue_source_exists_by_scan(
        &self,
        _customer_ref: &str,
        marker: &str,
    ) -> Result<bool, WerkaPortError> {
        assert!(marker.contains("accord_customer_issue_source:"));
        assert!(marker.contains("source_barcode=30AD3353F0C879E4801AD4DF"));
        assert!(marker.contains("source_stock_entry=MAT-STE-2026-00572"));
        assert!(marker.contains("source_line_index=1"));
        Ok(matches!(self.mode, FakeIssueMode::Duplicate))
    }

    async fn create_draft_delivery_note(
        &self,
        input: CreateDeliveryNoteInput,
    ) -> Result<String, WerkaPortError> {
        assert_eq!(input.customer, "CUST-001");
        assert_eq!(input.item_code, "ITEM-001");
        assert_eq!(input.qty, 2.0);
        assert!(input.source_key.contains("source_line_index=1"));
        Ok("DN-001".to_string())
    }

    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "DN-001");
        assert_eq!(update.flow_state, "1");
        assert_eq!(update.customer_state, "1");
        assert_eq!(update.delivery_actor, "1");
        assert_eq!(update.ui_status, "pending");
        Ok(())
    }

    async fn submit_delivery_note(&self, name: &str) -> Result<(), WerkaPortError> {
        assert_eq!(name, "DN-001");
        if matches!(self.mode, FakeIssueMode::InsufficientStock) {
            Err(WerkaPortError::InsufficientStock)
        } else {
            Ok(())
        }
    }

    async fn delete_delivery_note(&self, name: &str) -> Result<(), WerkaPortError> {
        assert_eq!(name, "DN-001");
        Ok(())
    }
}
