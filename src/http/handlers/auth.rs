use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::{LoginRequest, LoginResponse, Principal, PrincipalRole};
use crate::core::auth::service::AuthError;

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut principal = state
        .auth
        .login(request.phone.trim(), request.code.trim())
        .await
        .map_err(login_error)?;
    principal = state.profiles.refresh(principal).await;
    let token = state
        .sessions
        .create(principal.clone())
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "session create failed",
                }),
            )
        })?;

    Ok(Json(LoginResponse {
        profile: with_avatar_proxy(&headers, principal, &token),
        token,
        werka_home: None,
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<OkResponse>, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    state.sessions.delete(&token).await;

    Ok(Json(OkResponse { ok: true }))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Principal>, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(&headers).ok_or_else(unauthorized)?;
    let mut principal = state
        .sessions
        .get(&token)
        .await
        .map_err(|_| unauthorized())?;
    principal = state.profiles.refresh(principal).await;
    state.sessions.update(&token, principal.clone()).await;

    Ok(Json(with_avatar_proxy(&headers, principal, &token)))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = raw.strip_prefix("Bearer ")?.trim();

    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn with_avatar_proxy(headers: &HeaderMap, mut principal: Principal, token: &str) -> Principal {
    if principal.role != PrincipalRole::Supplier
        || principal.ref_.trim().is_empty()
        || principal.avatar_url.trim().is_empty()
    {
        return principal;
    }

    let Some(host) = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return principal;
    };

    principal.avatar_url = format!(
        "{}://{}/v1/mobile/profile/avatar/view?token={}",
        request_scheme(headers),
        host,
        urlencoding::encode(token.trim())
    );
    principal
}

fn request_scheme(headers: &HeaderMap) -> &str {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| value.eq_ignore_ascii_case("https"))
        .map(|_| "https")
        .unwrap_or("http")
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

fn login_error(error: AuthError) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        AuthError::InvalidCredentials | AuthError::InvalidRole => (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid credentials",
            }),
        ),
        AuthError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "internal error",
            }),
        ),
    }
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

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::with_avatar_proxy;
    use crate::core::auth::models::{Principal, PrincipalRole};

    #[test]
    fn supplier_avatar_uses_token_proxy_url() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("mobile.test"));

        let principal = with_avatar_proxy(
            &headers,
            Principal {
                role: PrincipalRole::Supplier,
                display_name: "Supplier".to_string(),
                legal_name: "Supplier".to_string(),
                ref_: "SUP-001".to_string(),
                phone: "+998901234567".to_string(),
                avatar_url: "http://erp.test/files/avatar.png".to_string(),
            },
            "abc token",
        );

        assert_eq!(
            principal.avatar_url,
            "http://mobile.test/v1/mobile/profile/avatar/view?token=abc%20token"
        );
    }

    #[test]
    fn customer_avatar_is_not_proxied() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("mobile.test"));

        let principal = with_avatar_proxy(
            &headers,
            Principal {
                role: PrincipalRole::Customer,
                display_name: "Customer".to_string(),
                legal_name: "Customer".to_string(),
                ref_: "CUST-001".to_string(),
                phone: "+998901234567".to_string(),
                avatar_url: "http://erp.test/files/avatar.png".to_string(),
            },
            "token",
        );

        assert_eq!(principal.avatar_url, "http://erp.test/files/avatar.png");
    }
}
