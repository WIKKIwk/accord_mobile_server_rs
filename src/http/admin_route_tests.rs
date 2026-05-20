use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminState};
use crate::core::admin::ports::{
    AdminCredentialPort, AdminPortError, AdminReadPort, AdminStatePort, AdminWritePort,
};
use crate::core::admin::service::AdminService;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::{DispatchRecord, SupplierItem};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;

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
async fn admin_method_checks_happen_after_auth_like_go() {
    let state = test_state();
    let cases = [
        ("PATCH", "/v1/mobile/admin/settings"),
        ("PATCH", "/v1/mobile/admin/suppliers"),
        ("POST", "/v1/mobile/admin/suppliers/list"),
        ("POST", "/v1/mobile/admin/suppliers/summary"),
        ("POST", "/v1/mobile/admin/suppliers/detail"),
        ("POST", "/v1/mobile/admin/suppliers/inactive"),
        ("POST", "/v1/mobile/admin/suppliers/items/assigned"),
        ("POST", "/v1/mobile/admin/suppliers/status"),
        ("POST", "/v1/mobile/admin/suppliers/phone"),
        ("POST", "/v1/mobile/admin/suppliers/items"),
        ("GET", "/v1/mobile/admin/suppliers/items/add"),
        ("GET", "/v1/mobile/admin/suppliers/items/remove"),
        ("GET", "/v1/mobile/admin/suppliers/code/regenerate"),
        ("GET", "/v1/mobile/admin/suppliers/remove"),
        ("GET", "/v1/mobile/admin/suppliers/restore"),
        ("PATCH", "/v1/mobile/admin/customers"),
        ("POST", "/v1/mobile/admin/customers/list"),
        ("POST", "/v1/mobile/admin/customers/detail"),
        ("POST", "/v1/mobile/admin/customers/phone"),
        ("GET", "/v1/mobile/admin/customers/code/regenerate"),
        ("GET", "/v1/mobile/admin/customers/items/add"),
        ("GET", "/v1/mobile/admin/customers/items/remove"),
        ("GET", "/v1/mobile/admin/customers/remove"),
        ("PATCH", "/v1/mobile/admin/items"),
        ("GET", "/v1/mobile/admin/items/bulk-move-group"),
        ("PATCH", "/v1/mobile/admin/item-groups"),
        ("POST", "/v1/mobile/admin/item-groups/tree"),
        ("POST", "/v1/mobile/admin/activity"),
        ("GET", "/v1/mobile/admin/werka/code/regenerate"),
    ];

    let supplier_token = session(&state, PrincipalRole::Supplier).await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    for (method, path) in cases {
        let unauthorized = build_router(state.clone())
            .oneshot(request(method, path, ""))
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(json_body(unauthorized).await["error"], "unauthorized");

        let forbidden = build_router(state.clone())
            .oneshot(request(method, path, &supplier_token))
            .await
            .expect("response");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(json_body(forbidden).await["error"], "forbidden");

        let method_not_allowed = build_router(state.clone())
            .oneshot(request(method, path, &admin_token))
            .await
            .expect("response");
        assert_eq!(
            method_not_allowed.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{path}"
        );
        assert_eq!(
            json_body(method_not_allowed).await["error"],
            "method not allowed"
        );
    }
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
async fn admin_settings_ignores_state_read_failure_like_go() {
    let mut state = test_state();
    let erp = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(erp.clone())
        .with_write_port(erp)
        .with_state_port(Arc::new(FailingAdminStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["werka_code_locked"], false);
    assert_eq!(value["werka_code_retry_after_sec"], 0);
}

#[tokio::test]
async fn admin_suppliers_summary_failure_uses_go_error_text() {
    let mut state = test_state();
    let erp = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(erp.clone())
        .with_write_port(erp)
        .with_state_port(Arc::new(FailingAdminStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier summary failed"
    );
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
        .with_auth_config_sink(Arc::new(state.auth.clone()))
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
async fn admin_supplier_detail_uses_permission_fallback_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::permission()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
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
    assert_eq!(value["assigned_items"].as_array().expect("items").len(), 2);
    assert_eq!(value["assigned_items"][0]["code"], "ITEM-001");
}

#[tokio::test]
async fn admin_supplier_detail_does_not_fallback_on_non_permission_error() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::lookup_failed()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "supplier detail failed");
}

#[tokio::test]
async fn admin_assigned_supplier_items_permission_without_cache_is_empty_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::permission()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/items/assigned?ref=SUP-002",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("items")
            .is_empty()
    );
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

    let group_items = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/items?group=Products&limit=5",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(group_items.status(), StatusCode::OK);
    assert_eq!(json_body(group_items).await[0]["code"], "ITEM-001");

    let groups = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups", &token))
        .await
        .expect("response");
    assert_eq!(groups.status(), StatusCode::OK);
    assert_eq!(json_body(groups).await[0], "All Item Groups");
}

#[tokio::test]
async fn admin_item_group_tree_returns_parent_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups/tree", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["name"], "All Item Groups");
    assert_eq!(value[1]["name"], "Xomashyo");
    assert_eq!(value[1]["parent_item_group"], "All Item Groups");
    assert_eq!(value[2]["name"], "plyonka");
    assert_eq!(value[2]["parent_item_group"], "Xomashyo");
}

#[tokio::test]
async fn admin_item_group_create_returns_erpnext_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let group = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Kley","parent":"Kraska","is_group":false}"#,
        ))
        .await
        .expect("response");
    assert_eq!(group.status(), StatusCode::OK);
    let value = json_body(group).await;
    assert_eq!(value["name"], "Kley");
    assert_eq!(value["item_group_name"], "Kley");
    assert_eq!(value["parent_item_group"], "Kraska");
    assert_eq!(value["is_group"], false);

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"","parent":"All Item Groups","is_group":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid).await["error"],
        "item group name is required"
    );
}

#[tokio::test]
async fn admin_item_group_parent_move_returns_erpnext_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Xomashyo","parent":"All Item Groups"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(moved.status(), StatusCode::OK);
    let value = json_body(moved).await;
    assert_eq!(value["name"], "Xomashyo");
    assert_eq!(value["item_group_name"], "Xomashyo");
    assert_eq!(value["parent_item_group"], "All Item Groups");
    assert_eq!(value["is_group"], true);

    let invalid_root = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"All Item Groups","parent":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_root.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_root).await["error"],
        "root item group cannot be moved"
    );

    let invalid_cycle = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Xomashyo","parent":"Xomashyo"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_cycle.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_cycle).await["error"],
        "item group cannot be its own parent"
    );
}

#[tokio::test]
async fn admin_customer_detail_errors_are_500_like_go() {
    let mut state = test_state();
    let erp = Arc::new(CustomerItemsFailReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(erp)
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/detail?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "customer detail failed");
}

#[tokio::test]
async fn admin_customer_code_regenerate_cooldown_is_500_like_go() {
    let mut state = test_state();
    let erp = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(erp.clone())
        .with_write_port(erp)
        .with_state_port(Arc::new(LockedCustomerStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/customers/code/regenerate?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "customer code regenerate failed"
    );
}

#[tokio::test]
async fn admin_supplier_phone_not_found_is_404_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_write_port(Arc::new(MissingSupplierWritePort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/phone?ref=SUP-MISSING",
            &token,
            r#"{"phone":"+998901111111"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"], "supplier not found");
}

#[tokio::test]
async fn admin_supplier_phone_skips_write_for_removed_supplier_like_go() {
    let mut state = test_state();
    let writes = Arc::new(CountingSupplierWritePort::default());
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_write_port(writes.clone())
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/phone?ref=SUP-003",
            &token,
            r#"{"phone":"+998901111111"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"], "supplier not found");
    assert_eq!(writes.supplier_phone_updates.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admin_supplier_items_invalid_item_is_500_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(MissingItemsReadPort))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/items?ref=SUP-001",
            &token,
            r#"{"item_codes":["ITEM-MISSING"]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier items update failed"
    );
}

#[tokio::test]
async fn admin_activity_fails_without_history_provider_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/activity", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "admin activity failed");
}

#[tokio::test]
async fn admin_activity_limits_history_to_30_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(ActivityLookup));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/activity", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let items = value.as_array().expect("activity array");
    assert_eq!(items.len(), 30);
    assert_eq!(items[0]["id"], "REC-000");
    assert_eq!(items[29]["id"], "REC-029");
}

#[tokio::test]
async fn admin_settings_put_updates_auth_runtime_like_go() {
    let mut state = test_state();
    state.admin = state
        .admin
        .clone()
        .with_auth_config_sink(Arc::new(state.auth.clone()));
    let token = session(&state, PrincipalRole::Admin).await;

    let update = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/settings",
            &token,
            r#"{
                "erp_url":"https://erp.test",
                "erp_api_key":"key",
                "erp_api_secret":"secret",
                "default_target_warehouse":"Stores - CH",
                "default_uom":"Kg",
                "werka_phone":"+998881111111",
                "werka_name":"Updated Werka",
                "werka_code":"20UPDATED",
                "werka_code_locked":false,
                "werka_code_retry_after_sec":0,
                "admin_phone":"+998882222222",
                "admin_name":"Updated Admin"
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(update.status(), StatusCode::OK);

    let old = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998881111111","code":"20ABCDEF1234"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);

    let new = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998881111111","code":"20UPDATED"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(new.status(), StatusCode::OK);
    let value = json_body(new).await;
    assert_eq!(value["profile"]["role"], "werka");
    assert_eq!(value["profile"]["display_name"], "Updated Werka");
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
        catalog_cache_enabled: false,
        catalog_cache_fallback_direct_db: true,
        catalog_cache_path: std::path::PathBuf::from("data/catalog_cache.sqlite"),
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

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        Ok(vec![
            AdminItemGroup {
                name: "All Item Groups".to_string(),
                item_group_name: "All Item Groups".to_string(),
                parent_item_group: String::new(),
                is_group: true,
            },
            AdminItemGroup {
                name: "Xomashyo".to_string(),
                item_group_name: "Xomashyo".to_string(),
                parent_item_group: "All Item Groups".to_string(),
                is_group: true,
            },
            AdminItemGroup {
                name: "plyonka".to_string(),
                item_group_name: "plyonka".to_string(),
                parent_item_group: "Xomashyo".to_string(),
                is_group: true,
            },
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

struct CustomerItemsFailReadPort;

#[async_trait]
impl AdminReadPort for CustomerItemsFailReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}

struct AssignedItemsErrorReadPort {
    permission: bool,
}

impl AssignedItemsErrorReadPort {
    fn permission() -> Self {
        Self { permission: true }
    }

    fn lookup_failed() -> Self {
        Self { permission: false }
    }
}

#[async_trait]
impl AdminReadPort for AssignedItemsErrorReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_by_codes(item_codes).await
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        if self.permission {
            Err(AdminPortError::PermissionDenied)
        } else {
            Err(AdminPortError::LookupFailed)
        }
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

struct MissingItemsReadPort;

#[async_trait]
impl AdminReadPort for MissingItemsReadPort {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.suppliers_page(query, limit, offset).await
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.supplier_by_ref(ref_).await
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        FakeAdminReadPort.customers_page(query, limit, offset).await
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.customer_by_ref(ref_).await
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort.items_page(query, limit, offset).await
    }

    async fn items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        FakeAdminReadPort.item_groups(query, limit).await
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .assigned_supplier_items(supplier_ref, limit)
            .await
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        FakeAdminReadPort
            .customer_items(customer_ref, query, limit)
            .await
    }
}

struct ActivityLookup;

#[async_trait]
impl WerkaHomeLookup for ActivityLookup {
    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok((0..35)
            .map(|index| DispatchRecord {
                id: format!("REC-{index:03}"),
                supplier_name: "Supplier".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Rice".to_string(),
                uom: "Kg".to_string(),
                status: "confirmed".to_string(),
                created_label: "2026-02-08 12:00".to_string(),
                ..DispatchRecord::default()
            })
            .collect())
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

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        })
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group: true,
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

struct MissingSupplierWritePort;

#[async_trait]
impl AdminWritePort for MissingSupplierWritePort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_supplier_item(ref_, item_code)
            .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_customer_item(ref_, item_code)
            .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_customer_item(ref_, item_code)
            .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        FakeAdminReadPort
            .create_item(code, name, uom, item_group)
            .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort
            .create_item_group(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .update_item_group(item_code, item_group)
            .await
    }
}

#[derive(Default)]
struct CountingSupplierWritePort {
    supplier_phone_updates: AtomicUsize,
}

#[async_trait]
impl AdminWritePort for CountingSupplierWritePort {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        self.supplier_phone_updates.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_supplier_item(ref_, item_code)
            .await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        FakeAdminReadPort.create_customer(name, phone).await
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_phone(ref_, phone).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        FakeAdminReadPort.update_customer_code(ref_, code).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .assign_customer_item(ref_, item_code)
            .await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .unassign_customer_item(ref_, item_code)
            .await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        FakeAdminReadPort
            .create_item(code, name, uom, item_group)
            .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort
            .create_item_group(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        FakeAdminReadPort.move_item_group_parent(name, parent).await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        FakeAdminReadPort
            .update_item_group(item_code, item_group)
            .await
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

struct FailingAdminStatePort;

#[async_trait]
impl AdminStatePort for FailingAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}

struct LockedCustomerStatePort;

#[async_trait]
impl AdminStatePort for LockedCustomerStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(BTreeMap::from([(
            "CUST-001".to_string(),
            AdminState {
                custom_code: "30LOCKED".to_string(),
                cooldown_until: Some(time::OffsetDateTime::now_utc() + time::Duration::hours(1)),
                ..AdminState::default()
            },
        )]))
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
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
