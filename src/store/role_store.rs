use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::core::authz::{RoleDefinition, RoleDefinitionStorePort, RoleStoreError};
use crate::store::json_file::{read_map, write_pretty};

#[derive(Clone)]
pub struct RoleDefinitionStore {
    path: PathBuf,
    state: Arc<Mutex<RoleDefinitionStoreState>>,
}

#[derive(Default)]
struct RoleDefinitionStoreState {
    loaded: bool,
    roles: BTreeMap<String, RoleDefinition>,
}

impl RoleDefinitionStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Arc::new(Mutex::new(RoleDefinitionStoreState::default())),
        }
    }
}

#[async_trait]
impl RoleDefinitionStorePort for RoleDefinitionStore {
    async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        Ok(state.roles.values().cloned().collect())
    }

    async fn put_role_definition(&self, role: RoleDefinition) -> Result<(), RoleStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        state.roles.insert(role.id.clone(), role);
        write_pretty(&self.path, &state.roles)
            .await
            .map_err(|_| RoleStoreError::StoreFailed)
    }
}

async fn load_if_needed(
    path: &Path,
    state: &mut RoleDefinitionStoreState,
) -> Result<(), RoleStoreError> {
    if state.loaded {
        return Ok(());
    }
    state.roles = read_map::<RoleDefinition>(path)
        .await
        .map_err(|_| RoleStoreError::StoreFailed)?
        .into_iter()
        .collect();
    state.loaded = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::core::auth::models::PrincipalRole;
    use crate::core::authz::{RoleDefinition, RoleDefinitionStorePort};

    use super::RoleDefinitionStore;

    #[tokio::test]
    async fn role_definition_store_persists_custom_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roles.json");
        let store = RoleDefinitionStore::new(path.clone());

        store
            .put_role_definition(RoleDefinition {
                id: "scale_operator".to_string(),
                label: "Scale operator".to_string(),
                base_role: PrincipalRole::Werka,
                capability_codes: vec!["gscale.print".to_string()],
                system: false,
            })
            .await
            .expect("put role");
        drop(store);

        let reloaded = RoleDefinitionStore::new(path);
        let roles = reloaded.role_definitions().await.expect("role definitions");
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].id, "scale_operator");
        assert_eq!(roles[0].capability_codes, vec!["gscale.print"]);
    }
}
