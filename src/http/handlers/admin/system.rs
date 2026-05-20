use super::*;

pub async fn items_bulk_move_group(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AdminItemGroupBulkMoveResult>, AdminError> {
    authorize_admin(&state, &headers).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
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
    authorize_admin(&state, &headers).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    match state.admin.regenerate_werka_code().await {
        Ok(settings) => Ok(Json(settings)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(_) => Err(server_error("werka code regenerate failed")),
    }
}

pub async fn capabilities(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_admin(&state, &headers).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    Ok(json_response(capability_catalog_entries()))
}

pub(super) async fn authorize_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, AdminError> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    let principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    if has_capability(&principal, Capability::AdminAccess) {
        Ok(principal)
    } else {
        Err(forbidden())
    }
}
