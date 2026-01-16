use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::auth::ports::{AdminAccessState, AdminAccessStateLookup, AuthPortError};
use crate::store::json_file;

#[derive(Debug, Clone)]
pub struct AdminSupplierStateStore {
    path: PathBuf,
}

impl AdminSupplierStateStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl AdminAccessStateLookup for AdminSupplierStateStore {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        let raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;

        Ok(raw
            .into_iter()
            .map(|(key, value)| {
                (
                    key,
                    AdminAccessState {
                        custom_code: value.custom_code,
                        blocked: value.blocked,
                        removed: value.removed,
                    },
                )
            })
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct AdminSupplierStateRecord {
    #[serde(default)]
    custom_code: String,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    removed: bool,
}

#[cfg(test)]
mod tests {
    use crate::core::auth::ports::AdminAccessStateLookup;

    use super::AdminSupplierStateStore;

    #[tokio::test]
    async fn reads_go_admin_supplier_state_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("admin.json");
        tokio::fs::write(
            &path,
            r#"{"SUP-001":{"custom_code":"10CUSTOM","blocked":true,"removed":false}}"#,
        )
        .await
        .expect("write state");

        let states = AdminSupplierStateStore::new(path)
            .list_states()
            .await
            .expect("states");

        let state = states.get("SUP-001").expect("supplier state");
        assert_eq!(state.custom_code, "10CUSTOM");
        assert!(state.blocked);
        assert!(!state.removed);
    }
}
