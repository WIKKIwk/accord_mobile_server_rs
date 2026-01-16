use axum::Json;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use serde::Deserialize;
use time::{Date, Month};

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::{
    CustomerDirectoryEntry, DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse,
    WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
};
use crate::http::archive_pdf::build_archive_pdf;
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

#[derive(Debug, Deserialize)]
pub struct DirectoryQuery {
    q: Option<String>,
    limit: Option<String>,
    offset: Option<String>,
}

pub async fn suppliers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<SupplierDirectoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 200, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state.werka.suppliers(q, limit, offset).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka suppliers failed",
            }),
        )),
    }
}

pub async fn customers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DirectoryQuery>,
) -> Result<Json<Vec<CustomerDirectoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    let q = query.q.as_deref().unwrap_or("").trim();
    let limit = optional_search_limit(query.limit.as_deref(), 200, 200);
    let offset = optional_search_offset(query.offset.as_deref());
    match state.werka.customers(q, limit, offset).await {
        Ok(Some(items)) => Ok(Json(items)),
        Ok(None) | Err(_) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "werka customers failed",
            }),
        )),
    }
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

pub async fn archive_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArchiveQuery>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_werka(&principal)?;

    let from = parse_archive_date(query.from.as_deref()).map_err(|_| archive_pdf_failed())?;
    let to = parse_archive_date(query.to.as_deref()).map_err(|_| archive_pdf_failed())?;
    let kind = query.kind.as_deref().unwrap_or("").trim();
    let period = query.period.as_deref().unwrap_or("").trim();
    let data = match state.werka.archive(kind, period, from, to).await {
        Ok(Some(data)) => data,
        Ok(None) | Err(_) => return Err(archive_pdf_failed()),
    };

    let filename = format!("werka-{}-{}.pdf", data.kind, data.period);
    let mut response = Response::new(Body::from(build_archive_pdf(&data)));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| archive_pdf_failed())?,
    );
    Ok(response)
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

fn archive_pdf_failed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "werka archive pdf failed",
        }),
    )
}

fn optional_search_limit(raw: Option<&str>, default_limit: usize, max_limit: usize) -> usize {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return default_limit;
    };
    let Ok(value) = trimmed.parse::<usize>() else {
        return default_limit;
    };
    if value == 0 {
        return default_limit;
    }
    if max_limit > 0 && value > max_limit {
        return max_limit;
    }
    value
}

fn optional_search_offset(raw: Option<&str>) -> usize {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return 0;
    };
    trimmed.parse::<usize>().unwrap_or(0)
}
