use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::gscale::{GscaleServiceError, MaterialReceiptPrintRequest};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub async fn material_receipt_print(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GscaleErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    if !matches!(principal.role, PrincipalRole::Admin | PrincipalRole::Werka) {
        return Err(forbidden());
    }
    let request: MaterialReceiptPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let response = state
        .gscale
        .print_material_receipt(request)
        .await
        .map_err(gscale_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<GscaleErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

fn gscale_error(error: GscaleServiceError) -> (StatusCode, Json<GscaleErrorResponse>) {
    let status = match error {
        GscaleServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        GscaleServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        GscaleServiceError::EpcGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        GscaleServiceError::DuplicateBarcodeRetriesExhausted { .. } => StatusCode::CONFLICT,
        GscaleServiceError::ErpWrite(_)
        | GscaleServiceError::PrintFailed { .. }
        | GscaleServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (
        status,
        Json(GscaleErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn unauthorized() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(GscaleErrorResponse::new("unauthorized", "unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(GscaleErrorResponse::new("forbidden", "forbidden")),
    )
}

fn bad_request(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(GscaleErrorResponse::new(error, detail)),
    )
}

fn method_not_allowed() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(GscaleErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
}

#[derive(Debug, Serialize)]
pub struct GscaleErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

impl GscaleErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}

#[allow(dead_code)]
fn _keeps_error_response_compatible(_response: ErrorResponse) {}
