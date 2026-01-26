use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::admin::models::AdminState;
use crate::core::admin::ports::{AdminPortError, AdminStatePort};
use crate::core::auth::ports::{AdminAccessState, AdminAccessStateLookup, AuthPortError};
use crate::core::werka::ports::{
    WerkaPortError, WerkaSupplierAdminState, WerkaSupplierAdminStateLookup,
};
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
impl AdminStatePort for AdminSupplierStateStore {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        let raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(raw
            .into_iter()
            .map(|(key, value)| {
                (
                    key,
                    AdminState {
                        custom_code: value.custom_code,
                        blocked: value.blocked,
                        removed: value.removed,
                        assigned_item_codes: value.assigned_item_codes,
                        cooldown_until: value.cooldown_until,
                        regen_window_started_at: value.regen_window_started_at,
                        regen_window_count: value.regen_window_count,
                        pending_persist_code: value.pending_persist_code,
                        pending_persist_at: value.pending_persist_at,
                        assignments_configured: value.assignments_configured,
                    },
                )
            })
            .collect())
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        let mut raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        raw.insert(
            ref_.trim().to_string(),
            AdminSupplierStateRecord {
                custom_code: state.custom_code,
                blocked: state.blocked,
                removed: state.removed,
                assignments_configured: state.assignments_configured,
                assigned_item_codes: state.assigned_item_codes,
                pending_persist_code: state.pending_persist_code,
                pending_persist_at: state.pending_persist_at,
                regen_window_started_at: state.regen_window_started_at,
                regen_window_count: state.regen_window_count,
                cooldown_until: state.cooldown_until,
            },
        );
        json_file::write_pretty(&self.path, &raw)
            .await
            .map_err(|_| AdminPortError::LookupFailed)
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

#[async_trait]
impl WerkaSupplierAdminStateLookup for AdminSupplierStateStore {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError> {
        let raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| WerkaPortError::LookupFailed)?;
        let Some(state) = raw.get(supplier_ref.trim()) else {
            return Ok(WerkaSupplierAdminState::default());
        };
        Ok(WerkaSupplierAdminState {
            blocked: state.blocked,
            removed: state.removed,
            assigned_item_codes: state.assigned_item_codes.clone(),
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AdminSupplierStateRecord {
    #[serde(default)]
    custom_code: String,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    removed: bool,
    #[serde(default)]
    assignments_configured: bool,
    #[serde(default)]
    assigned_item_codes: Vec<String>,
    #[serde(default)]
    pending_persist_code: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pending_persist_at: Option<time::OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    regen_window_started_at: Option<time::OffsetDateTime>,
    #[serde(default)]
    regen_window_count: i32,
    #[serde(default, with = "time::serde::rfc3339::option")]
    cooldown_until: Option<time::OffsetDateTime>,
}

#[cfg(test)]
mod tests {
    use crate::core::auth::ports::AdminAccessStateLookup;
    use crate::core::werka::ports::WerkaSupplierAdminStateLookup;

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

    #[tokio::test]
    async fn reads_go_assigned_item_codes_for_werka_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("admin.json");
        tokio::fs::write(
            &path,
            r#"{"SUP-001":{"assigned_item_codes":["ITEM-001","ITEM-002"],"blocked":false,"removed":false}}"#,
        )
        .await
        .expect("write state");

        let state = AdminSupplierStateStore::new(path)
            .werka_supplier_admin_state("SUP-001")
            .await
            .expect("state");

        assert_eq!(state.assigned_item_codes, ["ITEM-001", "ITEM-002"]);
        assert!(!state.blocked);
        assert!(!state.removed);
    }
}
