use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use time::{Date, Month};

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::{
    DispatchRecord, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

#[derive(Debug, Deserialize)]
pub struct StatusBreakdownQuery {
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StatusDetailsQuery {
    kind: Option<String>,
    supplier_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveQuery {
    kind: Option<String>,
    period: Option<String>,
    from: Option<String>,
    to: Option<String>,
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

pub async fn status_details(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StatusDetailsQuery>,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    let kind = query.kind.as_deref().unwrap_or("").trim();
    let supplier_ref = query.supplier_ref.as_deref().unwrap_or("").trim();
    match state.werka.status_details(kind, supplier_ref).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka status details failed",
            }),
        )),
    }
}

pub async fn archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Result<Json<WerkaArchiveResponse>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    let from = parse_archive_date(query.from.as_deref())?;
    let to = parse_archive_date(query.to.as_deref())?;
    let kind = query.kind.as_deref().unwrap_or("").trim();
    let period = query.period.as_deref().unwrap_or("").trim();
    match state.werka.archive(kind, period, from, to).await {
        Ok(Some(data)) => Ok(Json(data)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka archive failed",
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

fn parse_archive_date(
    raw: Option<&str>,
) -> Result<Option<Date>, (StatusCode, Json<ErrorResponse>)> {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let parts: Vec<_> = trimmed.split('-').collect();
    if parts.len() != 3 {
        return Err(archive_failed());
    }
    let year = parts[0].parse::<i32>().map_err(|_| archive_failed())?;
    let month = parts[1].parse::<u8>().map_err(|_| archive_failed())?;
    let day = parts[2].parse::<u8>().map_err(|_| archive_failed())?;
    let month = Month::try_from(month).map_err(|_| archive_failed())?;
    Date::from_calendar_date(year, month, day)
        .map(Some)
        .map_err(|_| archive_failed())
}

fn archive_failed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "werka archive failed",
        }),
    )
}
