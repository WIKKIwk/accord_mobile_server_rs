use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::{LoginRequest, LoginResponse, Principal};

pub async fn login(
    State(_state): State<AppState>,
    Json(_request): Json<LoginRequest>,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ErrorResponse {
            error: "login not implemented yet",
        }),
    )
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<OkResponse>) {
    if let Some(token) = bearer_token(&headers) {
        state.sessions.delete(&token).await;
    }

    (StatusCode::OK, Json(OkResponse { ok: true }))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Principal>, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    let principal = state.sessions.get(&token).await.map_err(|_| unauthorized())?;

    Ok(Json(principal))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = raw.strip_prefix("Bearer ")?.trim();

    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[allow(dead_code)]
fn _login_response_contract(_response: LoginResponse) {}
