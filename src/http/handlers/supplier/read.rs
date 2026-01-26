use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use super::authz::{authorize, require_supplier};
use crate::app::AppState;
use crate::core::werka::models::{DispatchRecord, SupplierHomeSummary};
use crate::http::handlers::auth::ErrorResponse;

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SupplierHomeSummary>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&principal)?;

    match state
        .werka
        .supplier_summary(&principal.ref_, &principal.display_name)
        .await
    {
        Ok(Some(summary)) => Ok(Json(summary)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier summary failed",
            }),
        )),
    }
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_supplier(&principal)?;

    match state
        .werka
        .supplier_history(&principal.ref_, &principal.display_name)
        .await
    {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "supplier history failed",
            }),
        )),
    }
}
