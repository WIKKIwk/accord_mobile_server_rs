use std::sync::Arc;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::ports::{PushServiceError, PushTokenStorePort};

#[derive(Clone)]
pub struct PushService {
    store: Arc<dyn PushTokenStorePort>,
}

impl PushService {
    pub fn new(store: Arc<dyn PushTokenStorePort>) -> Self {
        Self { store }
    }

    #[cfg(test)]
    pub fn store_for_tests(&self) -> Arc<dyn PushTokenStorePort> {
        self.store.clone()
    }

    pub async fn register(
        &self,
        principal: &Principal,
        token: &str,
        platform: &str,
    ) -> Result<(), PushServiceError> {
        if token.trim().is_empty() {
            return Err(PushServiceError::TokenRequired);
        }
        self.store
            .move_token_to_key(&push_token_key(principal), token, platform)
            .await?;
        Ok(())
    }

    pub async fn delete(&self, principal: &Principal, token: &str) -> Result<(), PushServiceError> {
        if token.trim().is_empty() {
            return Err(PushServiceError::TokenRequired);
        }
        self.store.delete(&push_token_key(principal), token).await?;
        Ok(())
    }
}

pub fn push_token_key(principal: &Principal) -> String {
    format!("{}:{}", role_key(&principal.role), principal.ref_.trim())
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Admin => "admin",
    }
}
