use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::admin::models::{AdminDirectoryEntry, AdminState};
use crate::core::admin::ports::{
    AdminCredentialPort, AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort,
};
use crate::core::admin::service::AdminService;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::SupplierItem;

#[tokio::test]
async fn admin_settings_requires_admin_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn admin_settings_returns_config_shape_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["erp_url"], "https://erp.test");
    assert_eq!(value["default_uom"], "Kg");
    assert_eq!(value["werka_name"], "Werka");
    assert_eq!(value["admin_name"], "Admin");
}

#[tokio::test]
async fn admin_settings_put_uses_direct_credentials_and_default_uom_like_go() {
    let mut state = test_state();
    let erp = Arc::new(FakeAdminReadPort);
    let credentials = Arc::new(FakeAdminCredentialPort::new("db-key", "db-secret"));
    state.admin = AdminService::new(&state.config)
        .with_read_port(erp.clone())
        .with_write_port(erp)
        .with_state_port(Arc::new(FakeAdminStatePort::new()))
        .with_credential_port(credentials.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/settings",
            &token,
            r#"{
                "erp_url":"https://new-erp.test",
                "erp_api_key":"",
                "erp_api_secret":"",
                "default_target_warehouse":"Stores - NEW",
                "default_uom":"",
                "werka_phone":"+998881111111",
                "werka_name":"New Werka",
                "werka_code":"20NEW",
                "werka_code_locked":false,
                "werka_code_retry_after_sec":0,
                "admin_phone":"+998882222222",
                "admin_name":"New Admin"
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["erp_url"], "https://new-erp.test");
    assert_eq!(value["erp_api_key"], "db-key");
    assert_eq!(value["erp_api_secret"], "db-secret");
    assert_eq!(value["default_target_warehouse"], "Stores - NEW");
    assert_eq!(value["default_uom"], "Kg");
    assert_eq!(
        credentials.values().await,
        ("db-key".to_string(), "db-secret".to_string())
    );
}

#[tokio::test]
async fn admin_suppliers_page_filters_removed_and_counts_blocked_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["summary"]["total_suppliers"], 3);
    assert_eq!(value["summary"]["active_suppliers"], 1);
    assert_eq!(value["summary"]["blocked_suppliers"], 2);
    assert_eq!(value["suppliers"].as_array().expect("suppliers").len(), 2);
    assert_eq!(value["suppliers"][0]["ref"], "SUP-001");
    assert_eq!(value["suppliers"][0]["assigned_item_count"], 2);
    assert_eq!(value["customers"][0]["ref"], "CUST-001");
}

#[tokio::test]
async fn admin_supplier_detail_requires_ref_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers/detail", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "ref is required");
}

#[tokio::test]
async fn admin_supplier_detail_returns_assigned_items_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ref"], "SUP-001");
    assert_eq!(value["code"], "10CUSTOM");
    assert_eq!(value["assigned_items"][0]["code"], "ITEM-001");
}

#[tokio::test]
async fn admin_customers_and_items_read_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let customers = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/customers/list", &token))
        .await
        .expect("response");
    assert_eq!(customers.status(), StatusCode::OK);
    assert_eq!(json_body(customers).await[0]["ref"], "CUST-001");

    let items = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/items?q=rice&limit=5&offset=1",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(items.status(), StatusCode::OK);
    assert_eq!(json_body(items).await[0]["item_group"], "Products");

    let groups = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups", &token))
        .await
        .expect("response");
    assert_eq!(groups.status(), StatusCode::OK);
    assert_eq!(json_body(groups).await[0], "All Item Groups");
}

#[tokio::test]
async fn admin_create_supplier_and_customer_mutations_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let supplier = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/suppliers",
            &token,
            r#"{"name":"New Supplier","phone":"+998909999999"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(supplier.status(), StatusCode::OK);
    let value = json_body(supplier).await;
    assert_eq!(value["ref"], "SUP-NEW");
    assert_eq!(value["phone"], "+998909999999");

    let customer = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"New Customer","phone":"+998901234567"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(customer.status(), StatusCode::OK);
    let value = json_body(customer).await;
    assert_eq!(value["ref"], "CUST-NEW");
    assert_eq!(value["name"], "New Customer");
}

#[tokio::test]
async fn admin_supplier_status_and_remove_mutations_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let status = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/status?ref=SUP-001",
            &token,
            r#"{"blocked":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(json_body(status).await["blocked"], true);

    let remove = build_router(state)
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/suppliers/remove?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(remove.status(), StatusCode::OK);
    assert_eq!(json_body(remove).await["ok"], true);
}

#[tokio::test]
async fn admin_item_mutation_errors_match_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let missing = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/customers/items/remove?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing).await["error"],
        "ref and item_code are required"
    );

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items/bulk-move-group",
            &token,
            r#"{"item_codes":[],"item_group":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid).await["error"], "item codes are required");
}

#[tokio::test]
async fn admin_item_create_and_werka_regenerate_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let item = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-NEW","name":"New Item","uom":"Kg","item_group":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(item.status(), StatusCode::OK);
    let value = json_body(item).await;
    assert_eq!(value["code"], "ITEM-NEW");
    assert_eq!(value["item_group"], "Products");

    let settings = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/werka/code/regenerate",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(settings.status(), StatusCode::OK);
    let value = json_body(settings).await;
    assert!(
        value["werka_code"]
            .as_str()
            .expect("code")
            .starts_with("20")
    );
}

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        erp_url: "https://erp.test".to_string(),
        erp_api_key: "key".to_string(),
        erp_api_secret: "secret".to_string(),
        default_target_warehouse: "Stores - CH".to_string(),
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
    let erp = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(erp.clone())
        .with_write_port(erp)
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
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

fn request(method: &str, uri: &str, token: &str) -> Request<Body> {
    request_with_body(method, uri, token, "")
}

fn request_with_body(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
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

struct FakeAdminReadPort;

#[async_trait]
impl AdminReadPort for FakeAdminReadPort {
    async fn suppliers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(vec![
            entry("SUP-001", "Supplier One", "+998901111111"),
            entry("SUP-002", "Supplier Two", "+998902222222"),
            entry("SUP-003", "Supplier Removed", "+998903333333"),
        ])
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry(ref_, "Supplier One", "+998901111111"))
    }

    async fn customers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(vec![entry("CUST-001", "Customer One", "+998904444444")])
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry(ref_, "Customer One", "+998904444444"))
    }

    async fn items_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001")])
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(item_codes.iter().map(|code| item(code)).collect())
    }

    async fn item_groups(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        Ok(vec![
            "All Item Groups".to_string(),
            "All Item Groups".to_string(),
        ])
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001"), item("ITEM-002")])
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(vec![item("ITEM-001")])
    }
}

#[async_trait]
impl AdminWritePort for FakeAdminReadPort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("SUP-NEW", name, phone))
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn assign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn unassign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Ok(entry("CUST-NEW", name, phone))
    }

    async fn update_customer_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn update_customer_code(&self, _ref_: &str, _code: &str) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn assign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn unassign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        Ok(SupplierItem {
            code: code.to_string(),
            name: name.to_string(),
            uom: uom.to_string(),
            warehouse: "Stores - CH".to_string(),
            item_group: item_group.to_string(),
        })
    }

    async fn update_item_group(
        &self,
        _item_code: &str,
        _item_group: &str,
    ) -> Result<(), AdminPortError> {
        Ok(())
    }
}

struct FakeAdminStatePort {
    states: Mutex<BTreeMap<String, AdminState>>,
}

impl FakeAdminStatePort {
    fn new() -> Self {
        Self {
            states: Mutex::new(BTreeMap::from([
                (
                    "SUP-001".to_string(),
                    AdminState {
                        custom_code: "10CUSTOM".to_string(),
                        assigned_item_codes: vec!["ITEM-001".to_string(), "ITEM-002".to_string()],
                        ..AdminState::default()
                    },
                ),
                (
                    "SUP-002".to_string(),
                    AdminState {
                        blocked: true,
                        ..AdminState::default()
                    },
                ),
                (
                    "SUP-003".to_string(),
                    AdminState {
                        removed: true,
                        ..AdminState::default()
                    },
                ),
            ])),
        }
    }
}

#[async_trait]
impl AdminStatePort for FakeAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(self.states.lock().await.clone())
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.states.lock().await.insert(ref_.to_string(), state);
        Ok(())
    }
}

struct FakeAdminCredentialPort {
    values: Mutex<(String, String)>,
}

impl FakeAdminCredentialPort {
    fn new(api_key: &str, api_secret: &str) -> Self {
        Self {
            values: Mutex::new((api_key.to_string(), api_secret.to_string())),
        }
    }

    async fn values(&self) -> (String, String) {
        self.values.lock().await.clone()
    }
}

#[async_trait]
impl AdminCredentialPort for FakeAdminCredentialPort {
    async fn admin_api_auth(&self, _username: &str) -> Result<(String, String), AdminPortError> {
        Ok(self.values.lock().await.clone())
    }

    async fn update_admin_api_auth(
        &self,
        _username: &str,
        api_key: &str,
        api_secret: &str,
    ) -> Result<(), AdminPortError> {
        *self.values.lock().await = (api_key.to_string(), api_secret.to_string());
        Ok(())
    }
}

fn entry(ref_: &str, name: &str, phone: &str) -> AdminDirectoryEntry {
    AdminDirectoryEntry {
        ref_: ref_.to_string(),
        name: name.to_string(),
        phone: phone.to_string(),
    }
}

fn item(code: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: "Rice".to_string(),
        uom: "Kg".to_string(),
        warehouse: "Stores - CH".to_string(),
        item_group: "Products".to_string(),
    }
}
