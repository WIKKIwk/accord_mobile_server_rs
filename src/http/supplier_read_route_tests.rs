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
use crate::core::werka::models::{DispatchRecord, SupplierHomeSummary};
use crate::core::werka::ports::{SupplierReadLookup, WerkaPortError};
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
async fn supplier_summary_accepts_post_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_supplier_read_lookup(Arc::new(FakeSupplierRead));
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request("POST", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["pending_count"], 2);
    assert_eq!(value["submitted_count"], 1);
    assert_eq!(value["returned_count"], 3);
}

#[tokio::test]
async fn supplier_summary_forbids_non_supplier_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_supplier_read_lookup(Arc::new(FakeSupplierRead));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn supplier_summary_fails_without_provider_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier summary failed"
    );
}

#[tokio::test]
async fn supplier_history_accepts_post_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_supplier_read_lookup(Arc::new(FakeSupplierRead));
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request_to("POST", "/v1/mobile/supplier/history", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["id"], "PR-001");
    assert_eq!(value[0]["status"], "partial");
}

#[tokio::test]
async fn supplier_history_forbids_non_supplier_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_supplier_read_lookup(Arc::new(FakeSupplierRead));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(request_to("GET", "/v1/mobile/supplier/history", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn supplier_history_fails_without_provider_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request_to("GET", "/v1/mobile/supplier/history", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier history failed"
    );
}

fn request(method: &str, token: &str) -> Request<Body> {
    request_to(method, "/v1/mobile/supplier/summary", token)
}

fn request_to(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request")
}

async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

async fn supplier_session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901111111".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
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

struct FakeSupplierRead;

#[async_trait]
impl SupplierReadLookup for FakeSupplierRead {
    async fn supplier_summary(
        &self,
        supplier_ref: &str,
    ) -> Result<SupplierHomeSummary, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        Ok(SupplierHomeSummary {
            pending_count: 2,
            submitted_count: 1,
            returned_count: 3,
        })
    }

    async fn supplier_history(
        &self,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            record_type: "purchase_receipt".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Nos".to_string(),
            sent_qty: 5.0,
            accepted_qty: 3.0,
            status: "partial".to_string(),
            created_label: "2026-01-26".to_string(),
            ..DispatchRecord::default()
        }])
    }
}
