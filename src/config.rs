use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub session_store_path: PathBuf,
    pub session_ttl_seconds: Option<u64>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let addr = std::env::var("MOBILE_API_ADDR").unwrap_or_else(|_| ":8081".to_string());
        let session_path = std::env::var("MOBILE_API_SESSION_STORE")
            .unwrap_or_else(|_| "data/mobile_sessions.json".to_string());
        let ttl_hours = std::env::var("MOBILE_API_SESSION_TTL_HOURS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(24 * 30);

        Ok(Self {
            bind_addr: parse_bind_addr(&addr)?,
            session_store_path: PathBuf::from(session_path),
            session_ttl_seconds: Some(Duration::from_secs(ttl_hours * 60 * 60).as_secs()),
        })
    }
}

fn parse_bind_addr(raw: &str) -> Result<SocketAddr, AppError> {
    let trimmed = raw.trim();
    let normalized = if trimmed.starts_with(':') {
        format!("0.0.0.0{trimmed}")
    } else {
        trimmed.to_string()
    };

    normalized.parse().map_err(|_| AppError::InvalidConfig {
        key: "MOBILE_API_ADDR",
        value: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::parse_bind_addr;

    #[test]
    fn parses_go_style_bind_addr() {
        let addr = parse_bind_addr(":8081").expect("addr");

        assert_eq!(addr.to_string(), "0.0.0.0:8081");
    }
}
