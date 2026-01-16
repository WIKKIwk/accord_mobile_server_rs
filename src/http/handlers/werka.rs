use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::{
    DispatchRecord, WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

#[derive(Debug, Deserialize)]
pub struct StatusBreakdownQuery {
    kind: Option<String>,
}

pub async fn status_breakdown(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StatusBreakdownQuery>,
) -> Result<Json<Vec<WerkaStatusBreakdownEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    match state.werka.status_breakdown(kind).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka status breakdown failed",
            }),
        )),
    }
}

pub async fn pending(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    match state.werka.pending(0).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "pending fetch failed",
            }),
        )),
    }
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    match state.werka.history().await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "history fetch failed",
            }),
        )),
    }
}

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WerkaHomeSummary>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    match state.werka.summary().await {
        Ok(Some(summary)) => Ok(Json(summary)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka summary failed",
            }),
        )),
    }
}

pub async fn home(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WerkaHomeData>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    match state.werka.home(20).await {
        Ok(Some(data)) => Ok(Json(data)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka home failed",
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

fn require_werka(principal: &Principal) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if principal.role == PrincipalRole::Werka {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        ))
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
