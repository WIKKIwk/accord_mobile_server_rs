mod ai;
mod app;
mod config;
mod core;
mod erpdb;
mod erpnext;
mod error;
mod fcm;
#[cfg(test)]
mod fcm_tests;
mod http;
mod store;

use crate::app::AppState;
use crate::config::AppConfig;

#[tokio::main]
async fn main() -> Result<(), error::AppError> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = AppConfig::from_env()?;
    let bind_addr = config.bind_addr;
    let state = AppState::new(config);
    let app = http::router::build_router(state);

    tracing::info!(%bind_addr, "starting accord mobile server rs");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
