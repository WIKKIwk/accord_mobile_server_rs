use axum::extract::State;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::http::handlers::{auth, profile, werka};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/mobile/auth/login", post(auth::login))
        .route("/v1/mobile/auth/logout", post(auth::logout))
        .route("/v1/mobile/me", get(auth::me))
        .route("/v1/mobile/profile/avatar/view", get(profile::avatar_view))
        .route("/v1/mobile/werka/history", any(werka::history))
        .route("/v1/mobile/werka/pending", any(werka::pending))
        .route(
            "/v1/mobile/werka/status-breakdown",
            any(werka::status_breakdown),
        )
        .route("/v1/mobile/werka/summary", any(werka::summary))
        .route("/v1/mobile/werka/home", any(werka::home))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
}

async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    let _ = state.config.bind_addr;

    Json(HealthResponse { ok: true })
}
