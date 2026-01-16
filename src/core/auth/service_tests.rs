use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::models::PrincipalRole;
use super::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    SupplierLookup, SupplierRecord,
};
use super::service::{AuthService, normalize_phone};
use crate::config::AppConfig;

fn config() -> AppConfig {
    AppConfig {
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
    }
}

#[test]
fn normalizes_phone_like_go() {
    assert_eq!(normalize_phone("998901234567").unwrap(), "+998901234567");
    assert!(normalize_phone("+12345").is_err());
    assert!(normalize_phone("+998 90").is_err());
}

#[tokio::test]
async fn admin_login_does_not_need_erp() {
    let auth = AuthService::new(&config());
    let principal = auth
        .login("+998880000000", "19621978")
        .await
        .expect("admin login");

    assert_eq!(principal.role, PrincipalRole::Admin);
    assert_eq!(principal.ref_, "admin");
}

#[tokio::test]
async fn werka_login_is_code_driven() {
    let auth = AuthService::new(&config());
    let principal = auth
        .login("+123456789", "20ABCDEF1234")
        .await
        .expect("werka login");

    assert_eq!(principal.role, PrincipalRole::Werka);
    assert_eq!(principal.ref_, "werka");
}

#[tokio::test]
async fn supplier_login_uses_deterministic_code() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![SupplierRecord {
            id: "SUP-001".to_string(),
            name: "Abdulloh".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup::default());
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);

    let principal = auth
        .login("+998901234567", "104LJINSVVO5")
        .await
        .expect("supplier login");

    assert_eq!(principal.role, PrincipalRole::Supplier);
    assert_eq!(principal.ref_, "SUP-001");
}

#[tokio::test]
async fn supplier_login_respects_custom_code_and_blocked_state() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![
            SupplierRecord {
                id: "SUP-BLOCKED".to_string(),
                name: "Blocked".to_string(),
                phone: "+998901234567".to_string(),
            },
            SupplierRecord {
                id: "SUP-OK".to_string(),
                name: "Open".to_string(),
                phone: "+998901234567".to_string(),
            },
        ],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([
            (
                "SUP-BLOCKED".to_string(),
                AdminAccessState {
                    custom_code: "10CUSTOM".to_string(),
                    blocked: true,
                    removed: false,
                },
            ),
            (
                "SUP-OK".to_string(),
                AdminAccessState {
                    custom_code: "10CUSTOM".to_string(),
                    blocked: false,
                    removed: false,
                },
            ),
        ]),
    });
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);

    let principal = auth
        .login("+998901234567", "10CUSTOM")
        .await
        .expect("supplier login");

    assert_eq!(principal.ref_, "SUP-OK");
}

#[tokio::test]
async fn customer_login_requires_custom_code() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "CUST-001".to_string(),
            name: "Comfi".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "CUST-001".to_string(),
            AdminAccessState {
                custom_code: "30CUSTOM".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    let principal = auth
        .login("+998901234567", "30CUSTOM")
        .await
        .expect("customer login");

    assert_eq!(principal.role, PrincipalRole::Customer);
    assert_eq!(principal.ref_, "CUST-001");
}

#[tokio::test]
async fn customer_login_fails_without_custom_code() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "CUST-001".to_string(),
            name: "Comfi".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup::default());
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    assert!(auth.login("+998901234567", "30CUSTOM").await.is_err());
}

struct FakeSupplierLookup {
    suppliers: Vec<SupplierRecord>,
}

#[async_trait]
impl SupplierLookup for FakeSupplierLookup {
    async fn search_suppliers(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        Ok(self.suppliers.clone())
    }
}

struct FakeCustomerLookup {
    customers: Vec<CustomerRecord>,
}

#[async_trait]
impl CustomerLookup for FakeCustomerLookup {
    async fn search_customers(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        Ok(self.customers.clone())
    }
}

#[derive(Default)]
struct FakeStateLookup {
    states: BTreeMap<String, AdminAccessState>,
}

#[async_trait]
impl AdminAccessStateLookup for FakeStateLookup {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        Ok(self.states.clone())
    }
}
