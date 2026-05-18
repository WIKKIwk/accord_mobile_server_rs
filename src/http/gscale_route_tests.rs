use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::gscale::GscaleService;
use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, ScaleDriverPrintRequest,
    ScaleDriverPrintResponse,
};
use crate::core::gscale::ports::{
    EpcSource, GscalePortError, MaterialReceiptErpPort, ScaleDriverPort,
};
use crate::core::session::manager::SessionManager;
use crate::rps::RpsDriverClient;

#[tokio::test]
async fn material_receipt_print_requires_auth() {
    let response = build_router(test_state())
        .oneshot(request(
            "POST",
            "/v1/mobile/gscale/material-receipt/print",
            "",
            "{}",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn material_receipt_print_rejects_wrong_method() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/material-receipt/print",
            &token,
            "",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method_not_allowed");
}

#[tokio::test]
async fn material_receipt_print_runs_rs_transaction_flow() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_erp(Arc::new(FakeErp {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/gscale/material-receipt/print",
            &token,
            r#"{
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"zebra",
                "print_mode":"rfid",
                "gross_qty":2.5,
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "submitted");
    assert_eq!(body["draft_name"], "MAT-STE-ROUTE");
    assert_eq!(body["qty"], 1.72);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:1.720", "print", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_start_state_stop_is_persisted_by_rs() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label",
                "quantity_source":"scale",
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("start response");
    let started_body = json_body(started).await;

    assert_eq!(started_body["ok"], true);
    assert_eq!(started_body["batch"]["active"], true);
    assert_eq!(started_body["batch"]["id"], "batch-1");
    assert_eq!(started_body["batch"]["item_code"], "ITEM-1");
    assert_eq!(started_body["batch"]["warehouse"], "Stores - A");
    assert_eq!(started_body["batch"]["tare_kg"], 0.78);

    let current = router
        .clone()
        .oneshot(request("GET", "/v1/mobile/rps/batch/state", &token, ""))
        .await
        .expect("state response");
    let current_body = json_body(current).await;

    assert_eq!(current_body["batch"]["active"], true);
    assert_eq!(current_body["batch"]["item_name"], "Green Tea");

    let stopped = router
        .oneshot(request("POST", "/v1/mobile/rps/batch/stop", &token, ""))
        .await
        .expect("stop response");
    let stopped_body = json_body(stopped).await;

    assert_eq!(stopped_body["batch"]["active"], false);
    assert_eq!(stopped_body["batch"]["item_code"], "ITEM-1");
}

#[tokio::test]
async fn rps_batch_print_uses_active_rs_batch_and_transaction_flow() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_erp(Arc::new(FakeErp {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let _ = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-print-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"zebra",
                "print_mode":"rfid",
                "tare_enabled":true,
                "tare_kg":0.78
            }"#,
        ))
        .await
        .expect("start response");

    let printed = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let body = json_body(printed).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["item_code"], "ITEM-1");
    assert_eq!(body["warehouse"], "Stores - A");
    assert_eq!(body["gross_qty"], 2.5);
    assert_eq!(body["qty"], 1.72);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:1.720", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_print_returns_after_driver_without_waiting_for_erp_submit() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_erp(Arc::new(SlowErp {
            events: events.clone(),
            delay: Duration::from_millis(800),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }))
        .with_epc_source(Arc::new(FixedEpc("FAST-EPC-1")));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-fast-print-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ITEM-1",
                "item_name":"Green Tea",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("start response");
    assert_eq!(json_body(started).await["ok"], true);

    let started_at = Instant::now();
    let printed = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let elapsed = started_at.elapsed();
    let body = json_body(printed).await;

    assert!(
        elapsed < Duration::from_millis(500),
        "RPS print response took {elapsed:?}"
    );
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["epc"], "FAST-EPC-1");
    assert_eq!(body["item_code"], "ITEM-1");
    assert_eq!(body["warehouse"], "Stores - A");
    assert_eq!(events.lock().unwrap().as_slice(), ["print"]);

    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_print_returns_printed_before_late_erp_failure() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_erp(Arc::new(FailingSubmitErp {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{
                "client_batch_id":"batch-print-fail-1",
                "driver_url":"http://127.0.0.1:39117",
                "item_code":"ABCD Family",
                "item_name":"ABCD Family",
                "warehouse":"Stores - A",
                "printer":"godex",
                "print_mode":"label"
            }"#,
        ))
        .await
        .expect("start response");
    assert_eq!(json_body(started).await["ok"], true);

    let printed = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let status = printed.status();
    let body = json_body(printed).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["item_code"], "ABCD Family");
    assert_eq!(events.lock().unwrap().as_slice(), ["print"]);

    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["print", "create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn live_rps_batch_print_routes_through_rs_to_driver_when_env_is_set() {
    let driver_url = std::env::var("RPS_LIVE_DRIVER_URL").unwrap_or_default();
    if driver_url.trim().is_empty() {
        eprintln!("skipping live RPS driver test; set RPS_LIVE_DRIVER_URL");
        return;
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new()
        .with_erp(Arc::new(FakeErp {
            events: events.clone(),
        }))
        .with_driver(Arc::new(RpsDriverClient::new(
            Duration::from_secs(15),
            driver_url.clone(),
        )))
        .with_epc_source(Arc::new(FixedEpc("300833B2DDD90140000000A1")));
    let token = session(&state, PrincipalRole::Werka).await;
    let router = build_router(state);

    let started = router
        .clone()
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            &format!(
                r#"{{
                    "client_batch_id":"live-rps-driver-test",
                    "driver_url":"{}",
                    "item_code":"TEST-GODEX",
                    "item_name":"GoDEX RS Route Test",
                    "warehouse":"5070 Lab",
                    "printer":"godex",
                    "print_mode":"label",
                    "quantity_source":"scale"
                }}"#,
                driver_url.trim().trim_end_matches('/')
            ),
        ))
        .await
        .expect("start response");
    let started_body = json_body(started).await;
    assert_eq!(started_body["ok"], true);

    let printed = router
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5,"unit":"kg"}"#,
        ))
        .await
        .expect("print response");
    let status = printed.status();
    let body = json_body(printed).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["status"], "printed");
    assert_eq!(body["item_code"], "TEST-GODEX");
    assert_eq!(body["warehouse"], "5070 Lab");
    assert_eq!(body["printer"], "godex");
    assert_eq!(body["print_mode"], "label");
    assert_eq!(body["printer_status"], "sent");
    assert_eq!(body["gross_qty"], 2.5);
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["create:2.500", "submit:MAT-STE-ROUTE"]
    );
}

#[tokio::test]
async fn rps_batch_print_requires_active_batch() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/print",
            &token,
            r#"{"gross_qty":2.5}"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "batch_not_active");
}

#[tokio::test]
async fn rps_batch_start_requires_item_and_warehouse() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Werka).await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rps/batch/start",
            &token,
            r#"{"item_code":"ITEM-1"}"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "invalid_input");
}

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        erp_url: String::new(),
        erp_api_key: String::new(),
        erp_api_secret: String::new(),
        default_target_warehouse: String::new(),
        erp_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        admin_supplier_store_path: "data/mobile_admin_suppliers.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
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

async fn session(state: &AppState, role: PrincipalRole) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: "admin".to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.trim().is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

struct FakeErp {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl MaterialReceiptErpPort for FakeErp {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{:.3}", input.qty));
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-ROUTE".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct FailingSubmitErp {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl MaterialReceiptErpPort for FailingSubmitErp {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{:.3}", input.qty));
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-ROUTE".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Err(GscalePortError::ErpWrite(
            "NegativeStockError: insufficient stock".to_string(),
        ))
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct SlowErp {
    events: Arc<Mutex<Vec<String>>>,
    delay: Duration,
}

#[async_trait]
impl MaterialReceiptErpPort for SlowErp {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        tokio::time::sleep(self.delay).await;
        self.events
            .lock()
            .unwrap()
            .push(format!("create:{:.3}", input.qty));
        Ok(MaterialReceiptDraft {
            name: "MAT-STE-ROUTE".to_string(),
            item_code: input.item_code,
            warehouse: input.warehouse,
            qty: input.qty,
            uom: "Kg".to_string(),
            barcode: input.barcode,
        })
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Ok(())
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct FakeDriver {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ScaleDriverPort for FakeDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.events.lock().unwrap().push("print".to_string());
        Ok(ScaleDriverPrintResponse {
            ok: true,
            status: "done".to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            printer_status: "OK".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}

struct FixedEpc(&'static str);

impl EpcSource for FixedEpc {
    fn next_epc(&self) -> String {
        self.0.to_string()
    }
}
