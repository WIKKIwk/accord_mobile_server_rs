use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::models::PushTokenRegisterRequest;
use crate::core::push::ports::PushServiceError;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    Query(query): Query<PushTokenDeleteQuery>,
    body: Bytes,
) -> Result<Json<OkResponse>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_push_role(&principal)?;

    match method {
        Method::POST => {
            let request: PushTokenRegisterRequest =
                serde_json::from_slice(&body).map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "invalid json",
                        }),
                    )
                })?;
            state
                .push
                .register(&principal, &request.token, &request.platform)
                .await
                .map_err(register_error)?;
            Ok(Json(OkResponse { ok: true }))
        }
        Method::DELETE => {
            let token = query.token.as_deref().unwrap_or("").trim();
            state
                .push
                .delete(&principal, token)
                .await
                .map_err(delete_error)?;
            Ok(Json(OkResponse { ok: true }))
        }
        _ => Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        )),
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

fn require_push_role(principal: &Principal) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if principal.role == PrincipalRole::Supplier || principal.role == PrincipalRole::Werka {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        ))
    }
}

fn register_error(error: PushServiceError) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        PushServiceError::TokenRequired => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "token is required",
            }),
        ),
        PushServiceError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "push token save failed",
            }),
        ),
    }
}

fn delete_error(error: PushServiceError) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        PushServiceError::TokenRequired => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "token is required",
            }),
        ),
        PushServiceError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "push token delete failed",
            }),
        ),
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

#[derive(Debug, Deserialize)]
pub struct PushTokenDeleteQuery {
    pub token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}
