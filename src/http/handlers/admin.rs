use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::admin::models::{
    AdminBulkMoveItemsRequest, AdminCreateCustomerRequest, AdminCreateItemRequest,
    AdminCreateSupplierRequest, AdminCustomerDetail, AdminItemGroupBulkMoveResult,
    AdminPhoneUpdateRequest, AdminSettings, AdminSupplier, AdminSupplierDetail,
    AdminSupplierItemMutationRequest, AdminSupplierItemsUpdateRequest,
    AdminSupplierStatusUpdateRequest, AdminSupplierSummary,
};
use crate::core::admin::ports::AdminPortError;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::{CustomerDirectoryEntry, DispatchRecord, SupplierItem};
use crate::http::handlers::auth::bearer_token;

type AdminError = (StatusCode, Json<AdminErrorResponse>);

pub async fn settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AdminSettings>, AdminError> {
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    match method {
        Method::GET => state
            .admin
            .settings()
            .await
            .map(Json)
            .map_err(|_| server_error("settings fetch failed")),
        Method::PUT => {
            let input: AdminSettings = parse_json(&body)?;
            state
                .admin
                .update_settings(input)
                .await
                .map(Json)
                .map_err(|_| server_error("settings update failed"))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn suppliers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    match method {
        Method::GET => state
            .admin
            .suppliers_home()
            .await
            .map(json_response)
            .map_err(|_| server_error("suppliers fetch failed")),
        Method::POST => {
            let input: AdminCreateSupplierRequest = parse_json(&body)?;
            state
                .admin
                .create_supplier(&input.name, &input.phone)
                .await
                .map(json_response)
                .map_err(|_| server_error("supplier create failed"))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn supplier_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<AdminSupplier>>, AdminError> {
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
) -> Result<Json<AdminSupplierSummary>, AdminError> {
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
) -> Result<Json<AdminSupplierDetail>, AdminError> {
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
) -> Result<Json<Vec<AdminSupplier>>, AdminError> {
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
) -> Result<Json<Vec<SupplierItem>>, AdminError> {
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
    body: Bytes,
) -> Result<Response, AdminError> {
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    match method {
        Method::GET => state
            .admin
            .customers(500)
            .await
            .map(json_response)
            .map_err(|_| server_error("customers fetch failed")),
        Method::POST => {
            let input: AdminCreateCustomerRequest = parse_json(&body)?;
            state
                .admin
                .create_customer(&input.name, &input.phone)
                .await
                .map(json_response)
                .map_err(|_| server_error("customer create failed"))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn customer_list(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<CustomerDirectoryEntry>>, AdminError> {
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
) -> Result<Json<AdminCustomerDetail>, AdminError> {
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
    body: Bytes,
) -> Result<Response, AdminError> {
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    match method {
        Method::GET => state
            .admin
            .items_page(
                query.q.as_deref().unwrap_or(""),
                positive_int(query.limit.as_deref(), 50),
                optional_offset(query.offset.as_deref()),
            )
            .await
            .map(json_response)
            .map_err(|_| server_error("admin items failed")),
        Method::POST => {
            let input: AdminCreateItemRequest = parse_json(&body)?;
            state
                .admin
                .create_item(&input.code, &input.name, &input.uom, &input.item_group)
                .await
                .map(json_response)
                .map_err(|_| server_error("admin item create failed"))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn item_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
) -> Result<Json<Vec<String>>, AdminError> {
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
) -> Result<Json<Vec<DispatchRecord>>, AdminError> {
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

pub async fn customer_phone(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminPhoneUpdateRequest = parse_json(&body)?;
    state
        .admin
        .update_customer_phone(ref_, &input.phone)
        .await
        .map(Json)
        .map_err(|_| server_error("customer phone update failed"))
}

pub async fn customer_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.regenerate_customer_code(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer code regenerate failed")),
    }
}

pub async fn customer_item_add(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierItemMutationRequest = parse_json(&body)?;
    match state
        .admin
        .assign_customer_item(ref_, &input.item_code)
        .await
    {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer item add failed")),
    }
}

pub async fn customer_item_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefItemQuery>,
) -> Result<Json<AdminCustomerDetail>, AdminError> {
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let (ref_, item_code) = required_ref_item(query.ref_.as_deref(), query.item_code.as_deref())?;
    match state.admin.unassign_customer_item(ref_, item_code).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer item remove failed")),
    }
}

pub async fn customer_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<OkResponse>, AdminError> {
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.remove_customer(ref_).await {
        Ok(()) => Ok(Json(OkResponse { ok: true })),
        Err(AdminPortError::NotFound) => Err(not_found("customer not found")),
        Err(_) => Err(server_error("customer remove failed")),
    }
}

pub async fn supplier_status(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierStatusUpdateRequest = parse_json(&body)?;
    match state.admin.set_supplier_blocked(ref_, input.blocked).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier status failed")),
    }
}

pub async fn supplier_phone(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminPhoneUpdateRequest = parse_json(&body)?;
    state
        .admin
        .update_supplier_phone(ref_, &input.phone)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier phone update failed"))
}

pub async fn supplier_items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierItemsUpdateRequest = parse_json(&body)?;
    match state
        .admin
        .update_supplier_items(ref_, input.item_codes)
        .await
    {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
        Err(_) => Err(server_error("supplier items update failed")),
    }
}

pub async fn supplier_item_add(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
    body: Bytes,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    let input: AdminSupplierItemMutationRequest = parse_json(&body)?;
    state
        .admin
        .assign_supplier_item(ref_, &input.item_code)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier item add failed"))
}

pub async fn supplier_item_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefItemQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let (ref_, item_code) = required_ref_item(query.ref_.as_deref(), query.item_code.as_deref())?;
    state
        .admin
        .unassign_supplier_item(ref_, item_code)
        .await
        .map(Json)
        .map_err(|_| server_error("supplier item remove failed"))
}

pub async fn supplier_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.regenerate_supplier_code(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier code regenerate failed")),
    }
}

pub async fn supplier_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<OkResponse>, AdminError> {
    if method != Method::DELETE {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.remove_supplier(ref_).await {
        Ok(()) => Ok(Json(OkResponse { ok: true })),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier remove failed")),
    }
}

pub async fn supplier_restore(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<RefQuery>,
) -> Result<Json<AdminSupplierDetail>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let ref_ = required_ref(query.ref_.as_deref())?;
    match state.admin.restore_supplier(ref_).await {
        Ok(detail) => Ok(Json(detail)),
        Err(AdminPortError::NotFound) => Err(not_found("supplier not found")),
        Err(_) => Err(server_error("supplier restore failed")),
    }
}

pub async fn items_bulk_move_group(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AdminItemGroupBulkMoveResult>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    let input: AdminBulkMoveItemsRequest = parse_json(&body)?;
    match state
        .admin
        .move_items_to_group(input.item_codes, &input.item_group)
        .await
    {
        Ok(result) => Ok(Json(result)),
        Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
        Err(_) => Err(server_error("admin item bulk move failed")),
    }
}

pub async fn werka_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSettings>, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    authorize_admin(&state, &headers).await?;
    match state.admin.regenerate_werka_code().await {
        Ok(settings) => Ok(Json(settings)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(_) => Err(server_error("werka code regenerate failed")),
    }
}

async fn authorize_admin(state: &AppState, headers: &HeaderMap) -> Result<Principal, AdminError> {
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

fn required_ref(value: Option<&str>) -> Result<&str, AdminError> {
    let ref_ = value.unwrap_or("").trim();
    if ref_.is_empty() {
        Err(bad_request("ref is required"))
    } else {
        Ok(ref_)
    }
}

fn required_ref_item<'a>(
    ref_: Option<&'a str>,
    item_code: Option<&'a str>,
) -> Result<(&'a str, &'a str), AdminError> {
    let ref_ = ref_.unwrap_or("").trim();
    let item_code = item_code.unwrap_or("").trim();
    if ref_.is_empty() || item_code.is_empty() {
        Err(bad_request("ref and item_code are required"))
    } else {
        Ok((ref_, item_code))
    }
}

fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, AdminError> {
    serde_json::from_slice(body).map_err(|_| bad_request("invalid json"))
}

fn json_response<T: Serialize>(value: T) -> Response {
    Json(value).into_response()
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

fn unauthorized() -> AdminError {
    (
        StatusCode::UNAUTHORIZED,
        Json(AdminErrorResponse::new("unauthorized")),
    )
}

fn forbidden() -> AdminError {
    (
        StatusCode::FORBIDDEN,
        Json(AdminErrorResponse::new("forbidden")),
    )
}

fn method_not_allowed() -> AdminError {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(AdminErrorResponse::new("method not allowed")),
    )
}

fn bad_request(error: impl Into<String>) -> AdminError {
    (
        StatusCode::BAD_REQUEST,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

fn server_error(error: impl Into<String>) -> AdminError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

fn not_found(error: impl Into<String>) -> AdminError {
    (
        StatusCode::NOT_FOUND,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

fn too_many_requests(error: impl Into<String>) -> AdminError {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

#[derive(Serialize)]
pub struct AdminErrorResponse {
    pub error: String,
}

impl AdminErrorResponse {
    fn new(error: &'static str) -> Self {
        Self {
            error: error.to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
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
pub struct RefItemQuery {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    pub item_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemQuery {
    pub q: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
}
