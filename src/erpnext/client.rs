use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::Duration;

use crate::core::admin::ports::AdminErpConfigSink;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ErpnextClient {
    runtime: Arc<StdRwLock<ErpnextRuntimeConfig>>,
    pub(crate) http: reqwest::Client,
    pub(crate) delivery_note_state_fields_ensured: Arc<RwLock<bool>>,
}

#[derive(Debug, Clone, Default)]
struct ErpnextRuntimeConfig {
    base_url: String,
    api_key: String,
    api_secret: String,
    default_warehouse: String,
}

impl ErpnextClient {
    pub fn new(base_url: String, api_key: String, api_secret: String, timeout: Duration) -> Self {
        Self {
            runtime: Arc::new(StdRwLock::new(ErpnextRuntimeConfig {
                base_url: normalize_base_url(&base_url),
                api_key: api_key.trim().to_string(),
                api_secret: api_secret.trim().to_string(),
                default_warehouse: String::new(),
            })),
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
            delivery_note_state_fields_ensured: Arc::new(RwLock::new(false)),
        }
    }

    pub fn with_default_warehouse(self, default_warehouse: String) -> Self {
        self.runtime
            .write()
            .expect("erp runtime lock")
            .default_warehouse = default_warehouse.trim().to_string();
        self
    }

    pub(crate) fn base_url(&self) -> String {
        self.runtime
            .read()
            .expect("erp runtime lock")
            .base_url
            .clone()
    }

    pub(crate) fn default_warehouse(&self) -> String {
        self.runtime
            .read()
            .expect("erp runtime lock")
            .default_warehouse
            .clone()
    }

    pub(crate) fn auth_header(&self) -> String {
        let runtime = self.runtime.read().expect("erp runtime lock");
        format!("token {}:{}", runtime.api_key, runtime.api_secret)
    }
}

impl AdminErpConfigSink for ErpnextClient {
    fn set_erp_config(
        &self,
        base_url: &str,
        api_key: &str,
        api_secret: &str,
        default_warehouse: &str,
    ) {
        let mut runtime = self.runtime.write().expect("erp runtime lock");
        runtime.base_url = normalize_base_url(base_url);
        runtime.api_key = api_key.trim().to_string();
        runtime.api_secret = api_secret.trim().to_string();
        runtime.default_warehouse = default_warehouse.trim().to_string();
    }
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}
