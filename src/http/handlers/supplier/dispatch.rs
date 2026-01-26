use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};

use super::authz::{authorize, require_supplier};
use crate::app::AppState;
use crate::core::werka::models::{CreateDispatchRequest, DispatchRecord};
use crate::http::handlers::auth::ErrorResponse;

pub async fn create_dispatch(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<DispatchRecord>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let principal = authorize(&state, &headers).await?;
    require_supplier(&principal)?;

    let request: CreateDispatchRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid json",
            }),
        )
    })?;

    match state
        .werka
        .create_supplier_dispatch(
            &principal.ref_,
            &principal.display_name,
            &principal.phone,
            &request.item_code,
            request.qty,
        )
        .await
    {
        Ok(Some(record)) => Ok(Json(record)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "dispatch create failed",
            }),
        )),
    }
}
