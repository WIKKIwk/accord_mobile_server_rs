use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ErpnextClient {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) api_secret: String,
    pub(crate) default_warehouse: String,
    pub(crate) http: reqwest::Client,
    pub(crate) delivery_note_state_fields_ensured: Arc<RwLock<bool>>,
}

impl ErpnextClient {
    pub fn new(base_url: String, api_key: String, api_secret: String, timeout: Duration) -> Self {
        Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key: api_key.trim().to_string(),
            api_secret: api_secret.trim().to_string(),
            default_warehouse: String::new(),
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
            delivery_note_state_fields_ensured: Arc::new(RwLock::new(false)),
        }
    }

    pub fn with_default_warehouse(mut self, default_warehouse: String) -> Self {
        self.default_warehouse = default_warehouse.trim().to_string();
        self
    }

    pub(crate) fn auth_header(&self) -> String {
        format!("token {}:{}", self.api_key, self.api_secret)
    }
}
