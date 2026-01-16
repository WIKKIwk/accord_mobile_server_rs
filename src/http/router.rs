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
        .route("/v1/mobile/werka/archive", any(werka::archive))
        .route("/v1/mobile/werka/archive/pdf", any(werka::archive_pdf))
        .route(
            "/v1/mobile/werka/customer-issue/create",
            any(werka::customer_issue_create),
        )
        .route(
            "/v1/mobile/werka/customer-issue/batch-create",
            any(werka::customer_issue_batch_create),
        )
        .route("/v1/mobile/werka/history", any(werka::history))
        .route("/v1/mobile/werka/notifications", any(werka::history))
        .route("/v1/mobile/werka/pending", any(werka::pending))
        .route(
            "/v1/mobile/werka/status-breakdown",
            any(werka::status_breakdown),
        )
        .route(
            "/v1/mobile/werka/status-details",
            any(werka::status_details),
        )
        .route("/v1/mobile/werka/summary", any(werka::summary))
        .route(
            "/v1/mobile/werka/customer-item-options",
            any(werka::customer_item_options),
        )
        .route(
            "/v1/mobile/werka/customer-items",
            any(werka::customer_items),
        )
        .route(
            "/v1/mobile/werka/supplier-items",
            any(werka::supplier_items),
        )
        .route("/v1/mobile/werka/customers", any(werka::customers))
        .route("/v1/mobile/werka/suppliers", any(werka::suppliers))
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
