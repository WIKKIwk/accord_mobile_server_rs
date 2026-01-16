use axum::extract::State;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::http::handlers::{auth, profile, werka};

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/mobile/auth/login", post(auth::login))
        .route("/v1/mobile/auth/logout", post(auth::logout))
        .route("/v1/mobile/me", get(auth::me))
        .route("/v1/mobile/profile/avatar/view", get(profile::avatar_view))
        .route("/v1/mobile/werka/home", any(werka::home))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
}

async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    let _ = state.config.bind_addr;

    Json(HealthResponse { ok: true })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    use super::build_router;
    use crate::app::AppState;
    use crate::config::AppConfig;
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::session::manager::SessionManager;
    use crate::core::werka::models::{WerkaHomeData, WerkaHomeSummary};
    use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
    use crate::core::werka::service::WerkaService;

    fn test_state() -> AppState {
        let mut state = AppState::new(AppConfig {
            bind_addr: "127.0.0.1:8081".parse().expect("addr"),
            erp_url: String::new(),
            erp_api_key: String::new(),
            erp_api_secret: String::new(),
            erp_timeout: std::time::Duration::from_secs(15),
            session_store_path: "data/mobile_sessions.json".into(),
            admin_supplier_store_path: "data/mobile_admin_suppliers.json".into(),
            session_ttl_seconds: Some(30 * 24 * 60 * 60),
            supplier_prefix: "10".to_string(),
            werka_prefix: "20".to_string(),
            werka_code: "20ABCDEF1234".to_string(),
            werka_name: "Werka".to_string(),
            admin_phone: "+998880000000".to_string(),
            admin_name: "Admin".to_string(),
            admin_code: "19621978".to_string(),
            direct_read_enabled: false,
            direct_site_config_path: String::new(),
            direct_db_host: String::new(),
            direct_db_port: None,
            direct_db_user: String::new(),
            direct_db_password: String::new(),
            direct_db_name: String::new(),
        });
        state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
        state
    }

    #[tokio::test]
    async fn me_route_matches_go_contract() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/me")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_me_route_is_not_registered() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/auth/me")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn avatar_view_requires_auth() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/profile/avatar/view")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn avatar_view_forbids_non_supplier() {
        let state = test_state();
        let token = state
            .sessions
            .create(Principal {
                role: PrincipalRole::Customer,
                display_name: "Customer".to_string(),
                legal_name: "Customer".to_string(),
                ref_: "CUST-001".to_string(),
                phone: "+998901234567".to_string(),
                avatar_url: String::new(),
            })
            .await
            .expect("session");
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/profile/avatar/view")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn werka_home_requires_auth() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/werka/home")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn werka_home_forbids_non_werka() {
        let state = test_state();
        let token = state
            .sessions
            .create(Principal {
                role: PrincipalRole::Supplier,
                display_name: "Supplier".to_string(),
                legal_name: "Supplier".to_string(),
                ref_: "SUP-001".to_string(),
                phone: "+998901234567".to_string(),
                avatar_url: String::new(),
            })
            .await
            .expect("session");
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/werka/home")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn werka_home_fails_without_provider_like_go() {
        let state = test_state();
        let token = werka_session(&state).await;
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/werka/home")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn werka_home_returns_provider_payload() {
        let mut state = test_state();
        state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
        let token = werka_session(&state).await;
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/mobile/werka/home")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["summary"]["pending_count"], 2);
        assert_eq!(value["summary"]["confirmed_count"], 3);
        assert_eq!(value["summary"]["returned_count"], 1);
        assert_eq!(value["pending_items"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn werka_home_accepts_post_like_go_handler() {
        let mut state = test_state();
        state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
        let token = werka_session(&state).await;
        let app = build_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mobile/werka/home")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    async fn werka_session(state: &AppState) -> String {
        state
            .sessions
            .create(Principal {
                role: PrincipalRole::Werka,
                display_name: "Werka".to_string(),
                legal_name: "Werka".to_string(),
                ref_: "werka".to_string(),
                phone: "+99888862440".to_string(),
                avatar_url: String::new(),
            })
            .await
            .expect("session")
    }

    struct FakeWerkaHomeLookup;

    #[async_trait]
    impl WerkaHomeLookup for FakeWerkaHomeLookup {
        async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
            assert_eq!(pending_limit, 20);
            Ok(WerkaHomeData {
                summary: WerkaHomeSummary {
                    pending_count: 2,
                    confirmed_count: 3,
                    returned_count: 1,
                },
                pending_items: Vec::new(),
            })
        }
    }
}
