use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;

use crate::app::AppState;
use crate::core::admin::models::{
    AdminCustomerDetail, AdminSettings, AdminSupplier, AdminSupplierDetail, AdminSupplierSummary,
    AdminSuppliersPage,
};
use crate::core::admin::ports::AdminPortError;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::{CustomerDirectoryEntry, DispatchRecord, SupplierItem};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub async fn settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSettings>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .settings()
        .await
        .map(Json)
        .map_err(|_| server_error("settings fetch failed"))
}

pub async fn suppliers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSuppliersPage>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .suppliers_home()
        .await
        .map(Json)
        .map_err(|_| server_error("suppliers fetch failed"))
}

pub async fn supplier_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<AdminSupplier>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .suppliers_page(
            optional_search_limit(query.limit.as_deref(), 20, 50),
            optional_offset(query.offset.as_deref()),
        )
        .await
        .map(Json)
        .map_err(|_| server_error("suppliers page failed"))
}

pub async fn supplier_summary(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSupplierSummary>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .supplier_summary(300)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier summary failed"))
}

pub async fn supplier_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminSupplierDetail>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.supplier_detail(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier detail failed")),
    }
}

pub async fn inactive_suppliers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<Vec<AdminSupplier>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .inactive_suppliers(300)
        .await
        .map(Json)
        .map_err(|_| server_error("inactive suppliers failed"))
}

pub async fn assigned_supplier_items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<Vec<SupplierItem>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    state
        .admin
        .assigned_supplier_items(ref_, 200)
        .await
        .map(Json)
        .map_err(|_| server_error("assigned items fetch failed"))
}

pub async fn customers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<Vec<CustomerDirectoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .customers(500)
        .await
        .map(Json)
        .map_err(|_| server_error("customers fetch failed"))
}

pub async fn customer_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<CustomerDirectoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .customers_page(
            optional_search_limit(query.limit.as_deref(), 20, 50),
            optional_offset(query.offset.as_deref()),
        )
        .await
        .map(Json)
        .map_err(|_| server_error("customers page failed"))
}

pub async fn customer_detail(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminCustomerDetail>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.customer_detail(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer detail failed")),
    }
}

pub async fn items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
) -> Result<Json<Vec<SupplierItem>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .items_page(
            query.q.as_deref().unwrap_or(""),
            positive_int(query.limit.as_deref(), 50),
            optional_offset(query.offset.as_deref()),
        )
        .await
        .map(Json)
        .map_err(|_| server_error("admin items failed"))
}

pub async fn item_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
) -> Result<Json<Vec<String>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    state
        .admin
        .item_groups(query.q.as_deref().unwrap_or(""), 100)
        .await
        .map(Json)
        .map_err(|_| server_error("admin item groups failed"))
}

pub async fn activity(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<Vec<DispatchRecord>>, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let history = state.werka.history().await.ok().flatten();
    state
        .admin
        .activity(history)
        .await
        .map(Json)
        .map_err(|_| server_error("admin activity failed"))
}

async fn authorize_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    let principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    if principal.role == PrincipalRole::Admin {
        Ok(principal)
    } else {
        Err(forbidden())
    }
}

fn required_ref(value: Option<&str>) -> Result<&str, (StatusCode, Json<ErrorResponse>)> {
    let ref_ = value.unwrap_or("").trim();
    if ref_.is_empty() {
        Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "ref is required",
            }),
        ))
    } else {
        Ok(ref_)
    }
}

fn optional_search_limit(value: Option<&str>, default: usize, max: usize) -> usize {
    match value.unwrap_or("").trim().parse::<usize>() {
        Ok(limit) if limit > 0 && limit <= max => limit,
        _ => default,
    }
}

fn positive_int(value: Option<&str>, default: usize) -> usize {
    match value.unwrap_or("").trim().parse::<usize>() {
        Ok(value) if value > 0 => value,
        _ => default,
    }
}

fn optional_offset(value: Option<&str>) -> usize {
    value
        .unwrap_or("")
        .trim()
        .parse::<isize>()
        .ok()
        .filter(|value| *value >= 0)
        .unwrap_or(0) as usize
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

fn forbidden() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse { error: "forbidden" }),
    )
}

fn method_not_allowed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ErrorResponse {
            error: "method not allowed",
        }),
    )
}

fn server_error(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse { error }),
    )
}

fn not_found(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::NOT_FOUND, Json(ErrorResponse { error }))
}

#[derive(Debug, Deserialize)]
pub struct PageQuery {
    pub limit: Option<String>,
    pub offset: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RefQuery {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemQuery {
    pub q: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
}
