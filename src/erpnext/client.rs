use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ErpnextClient {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) api_secret: String,
    pub(crate) http: reqwest::Client,
}

impl ErpnextClient {
    pub fn new(base_url: String, api_key: String, api_secret: String, timeout: Duration) -> Self {
        Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key: api_key.trim().to_string(),
            api_secret: api_secret.trim().to_string(),
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("reqwest client"),
        }
    }

    pub(crate) fn auth_header(&self) -> String {
        format!("token {}:{}", self.api_key, self.api_secret)
    }
}
