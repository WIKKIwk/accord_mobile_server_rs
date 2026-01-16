use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::models::WerkaHomeData;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

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
