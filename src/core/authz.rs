use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Capability {
    AdminAccess,
    RoleCapabilityRead,
    RoleCapabilityManage,
    AdminSettingsRead,
    AdminSettingsManage,
    WerkaAccess,
    SupplierAccess,
    CustomerAccess,
    PushTokenManage,
    SupplierAvatarManage,
    CatalogItemRead,
    CatalogItemCreate,
    CatalogItemGroupRead,
    CatalogItemGroupManage,
    CatalogItemBulkMove,
    SupplierDirectoryRead,
    SupplierDirectoryManage,
    SupplierItemAssign,
    SupplierCodeManage,
    CustomerDirectoryRead,
    CustomerDirectoryManage,
    CustomerItemAssign,
    CustomerCodeManage,
    AdminActivityRead,
    WerkaCodeManage,
    GscaleCatalogRead,
    GscalePrint,
    RpsBatchManage,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CapabilityDefinition {
    pub capability: Capability,
    pub code: &'static str,
    pub label: &'static str,
    pub default_roles: &'static [PrincipalRole],
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct CapabilityCatalogEntry {
    pub code: &'static str,
    pub label: &'static str,
    pub default_roles: Vec<PrincipalRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDefinition {
    pub id: String,
    pub label: String,
    pub base_role: PrincipalRole,
    pub capability_codes: Vec<String>,
    pub system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RoleDefinitionUpsert {
    pub id: String,
    pub label: String,
    pub base_role: PrincipalRole,
    pub capability_codes: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RoleDefinitionError {
    #[error("role id is required")]
    MissingId,
    #[error("role label is required")]
    MissingLabel,
    #[error("role id is reserved")]
    ReservedId,
    #[error("role id is invalid")]
    InvalidId,
    #[error("role needs at least one capability")]
    MissingCapabilities,
    #[error("unknown capability: {0}")]
    UnknownCapability(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RoleStoreError {
    #[error("role store failed")]
    StoreFailed,
}

#[async_trait]
pub trait RoleDefinitionStorePort: Send + Sync {
    async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, RoleStoreError>;
    async fn put_role_definition(&self, role: RoleDefinition) -> Result<(), RoleStoreError>;
}

pub struct MemoryRoleDefinitionStore {
    roles: RwLock<BTreeMap<String, RoleDefinition>>,
}

impl MemoryRoleDefinitionStore {
    pub fn new() -> Self {
        Self {
            roles: RwLock::new(BTreeMap::new()),
        }
    }
}

impl Default for MemoryRoleDefinitionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RoleDefinitionStorePort for MemoryRoleDefinitionStore {
    async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, RoleStoreError> {
        Ok(self.roles.read().await.values().cloned().collect())
    }

    async fn put_role_definition(&self, role: RoleDefinition) -> Result<(), RoleStoreError> {
        self.roles.write().await.insert(role.id.clone(), role);
        Ok(())
    }
}

const ADMIN_ONLY: &[PrincipalRole] = &[PrincipalRole::Admin];
const WERKA_ONLY: &[PrincipalRole] = &[PrincipalRole::Werka];
const SUPPLIER_ONLY: &[PrincipalRole] = &[PrincipalRole::Supplier];
const CUSTOMER_ONLY: &[PrincipalRole] = &[PrincipalRole::Customer];
const SUPPLIER_WERKA: &[PrincipalRole] = &[PrincipalRole::Supplier, PrincipalRole::Werka];
const ADMIN_WERKA: &[PrincipalRole] = &[PrincipalRole::Admin, PrincipalRole::Werka];

const CAPABILITY_CATALOG: &[CapabilityDefinition] = &[
    CapabilityDefinition {
        capability: Capability::AdminAccess,
        code: "admin.access",
        label: "Admin panel",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::RoleCapabilityRead,
        code: "role.capability.read",
        label: "Role capability catalog read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::RoleCapabilityManage,
        code: "role.capability.manage",
        label: "Role capability manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::AdminSettingsRead,
        code: "admin.settings.read",
        label: "Admin settings read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::AdminSettingsManage,
        code: "admin.settings.manage",
        label: "Admin settings manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::WerkaAccess,
        code: "werka.access",
        label: "Werka workspace",
        default_roles: WERKA_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierAccess,
        code: "supplier.access",
        label: "Supplier workspace",
        default_roles: SUPPLIER_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerAccess,
        code: "customer.access",
        label: "Customer workspace",
        default_roles: CUSTOMER_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::PushTokenManage,
        code: "push.token.manage",
        label: "Push token manage",
        default_roles: SUPPLIER_WERKA,
    },
    CapabilityDefinition {
        capability: Capability::SupplierAvatarManage,
        code: "supplier.avatar.manage",
        label: "Supplier avatar manage",
        default_roles: SUPPLIER_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemRead,
        code: "catalog.item.read",
        label: "Catalog item read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemCreate,
        code: "catalog.item.create",
        label: "Catalog item create",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemGroupRead,
        code: "catalog.item_group.read",
        label: "Catalog item group read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemGroupManage,
        code: "catalog.item_group.manage",
        label: "Catalog item group manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CatalogItemBulkMove,
        code: "catalog.item.bulk_move",
        label: "Catalog item bulk move",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierDirectoryRead,
        code: "party.supplier.read",
        label: "Supplier directory read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierDirectoryManage,
        code: "party.supplier.manage",
        label: "Supplier directory manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierItemAssign,
        code: "party.supplier.item.assign",
        label: "Supplier item assign",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::SupplierCodeManage,
        code: "party.supplier.code.manage",
        label: "Supplier code manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerDirectoryRead,
        code: "party.customer.read",
        label: "Customer directory read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerDirectoryManage,
        code: "party.customer.manage",
        label: "Customer directory manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerItemAssign,
        code: "party.customer.item.assign",
        label: "Customer item assign",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::CustomerCodeManage,
        code: "party.customer.code.manage",
        label: "Customer code manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::AdminActivityRead,
        code: "admin.activity.read",
        label: "Admin activity read",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::WerkaCodeManage,
        code: "werka.code.manage",
        label: "Werka code manage",
        default_roles: ADMIN_ONLY,
    },
    CapabilityDefinition {
        capability: Capability::GscaleCatalogRead,
        code: "gscale.catalog.read",
        label: "GScale catalog read",
        default_roles: ADMIN_WERKA,
    },
    CapabilityDefinition {
        capability: Capability::GscalePrint,
        code: "gscale.print",
        label: "GScale print",
        default_roles: ADMIN_WERKA,
    },
    CapabilityDefinition {
        capability: Capability::RpsBatchManage,
        code: "rps.batch.manage",
        label: "RPS batch manage",
        default_roles: ADMIN_WERKA,
    },
];

pub fn capability_catalog() -> &'static [CapabilityDefinition] {
    CAPABILITY_CATALOG
}

pub fn capability_catalog_entries() -> Vec<CapabilityCatalogEntry> {
    capability_catalog()
        .iter()
        .map(|definition| CapabilityCatalogEntry {
            code: definition.code,
            label: definition.label,
            default_roles: definition.default_roles.to_vec(),
        })
        .collect()
}

pub fn capability_by_code(code: &str) -> Option<&'static CapabilityDefinition> {
    let code = code.trim();
    capability_catalog()
        .iter()
        .find(|definition| definition.code == code)
}

pub fn system_role_definitions() -> Vec<RoleDefinition> {
    [
        (PrincipalRole::Admin, "admin", "Admin"),
        (PrincipalRole::Werka, "werka", "Werka"),
        (PrincipalRole::Supplier, "supplier", "Supplier"),
        (PrincipalRole::Customer, "customer", "Customer"),
    ]
    .into_iter()
    .map(|(role, id, label)| RoleDefinition {
        id: id.to_string(),
        label: label.to_string(),
        capability_codes: capability_codes_for_role(role.clone()),
        base_role: role,
        system: true,
    })
    .collect()
}

pub fn normalize_custom_role(
    input: RoleDefinitionUpsert,
) -> Result<RoleDefinition, RoleDefinitionError> {
    let id = input.id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return Err(RoleDefinitionError::MissingId);
    }
    if system_role_ids().contains(id.as_str()) {
        return Err(RoleDefinitionError::ReservedId);
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(RoleDefinitionError::InvalidId);
    }

    let label = input.label.trim().to_string();
    if label.is_empty() {
        return Err(RoleDefinitionError::MissingLabel);
    }

    let requested: BTreeSet<String> = input
        .capability_codes
        .into_iter()
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
        .collect();
    if requested.is_empty() {
        return Err(RoleDefinitionError::MissingCapabilities);
    }
    for code in &requested {
        if capability_by_code(code).is_none() {
            return Err(RoleDefinitionError::UnknownCapability(code.clone()));
        }
    }

    let capability_codes = capability_catalog()
        .iter()
        .filter(|definition| requested.contains(definition.code))
        .map(|definition| definition.code.to_string())
        .collect();

    Ok(RoleDefinition {
        id,
        label,
        base_role: input.base_role,
        capability_codes,
        system: false,
    })
}

pub fn capabilities_for_role(role: PrincipalRole) -> Vec<Capability> {
    capability_catalog()
        .iter()
        .filter(|definition| definition.default_roles.contains(&role))
        .map(|definition| definition.capability)
        .collect()
}

pub fn capability_codes_for_role(role: PrincipalRole) -> Vec<String> {
    capability_catalog()
        .iter()
        .filter(|definition| definition.default_roles.contains(&role))
        .map(|definition| definition.code.to_string())
        .collect()
}

pub fn has_capability(principal: &Principal, capability: Capability) -> bool {
    capability_catalog()
        .iter()
        .find(|definition| definition.capability == capability)
        .map(|definition| definition.default_roles.contains(&principal.role))
        .unwrap_or(false)
}

fn system_role_ids() -> BTreeSet<&'static str> {
    ["admin", "werka", "supplier", "customer"]
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gscale_and_rps_capabilities_are_admin_or_werka_only() {
        let admin = principal(PrincipalRole::Admin);
        let werka = principal(PrincipalRole::Werka);
        let supplier = principal(PrincipalRole::Supplier);
        let customer = principal(PrincipalRole::Customer);

        for capability in [
            Capability::GscaleCatalogRead,
            Capability::GscalePrint,
            Capability::RpsBatchManage,
        ] {
            assert!(has_capability(&admin, capability));
            assert!(has_capability(&werka, capability));
            assert!(!has_capability(&supplier, capability));
            assert!(!has_capability(&customer, capability));
        }
    }

    #[test]
    fn default_role_capabilities_preserve_current_access_matrix() {
        let admin = principal(PrincipalRole::Admin);
        let werka = principal(PrincipalRole::Werka);
        let supplier = principal(PrincipalRole::Supplier);
        let customer = principal(PrincipalRole::Customer);

        assert!(has_capability(&admin, Capability::AdminAccess));
        assert!(has_capability(&werka, Capability::WerkaAccess));
        assert!(has_capability(&supplier, Capability::SupplierAccess));
        assert!(has_capability(&customer, Capability::CustomerAccess));

        assert!(has_capability(&supplier, Capability::PushTokenManage));
        assert!(has_capability(&werka, Capability::PushTokenManage));
        assert!(!has_capability(&customer, Capability::PushTokenManage));
        assert!(!has_capability(&admin, Capability::PushTokenManage));

        assert!(has_capability(&supplier, Capability::SupplierAvatarManage));
        assert!(!has_capability(&werka, Capability::SupplierAvatarManage));
    }

    #[test]
    fn capability_catalog_exposes_stable_codes_for_future_role_builder() {
        let catalog = capability_catalog();

        assert!(catalog.iter().any(|item| item.code == "admin.access"));
        assert!(catalog.iter().any(|item| item.code == "werka.access"));
        assert!(
            catalog
                .iter()
                .any(|item| item.code == "gscale.catalog.read")
        );
        assert!(catalog.iter().any(|item| item.code == "rps.batch.manage"));
        assert!(catalog.iter().all(|item| !item.label.trim().is_empty()));
    }

    #[test]
    fn capability_catalog_entries_are_serializable_for_admin_api() {
        let entries = capability_catalog_entries();
        let gscale = entries
            .iter()
            .find(|item| item.code == "gscale.catalog.read")
            .expect("gscale catalog capability");

        assert_eq!(
            gscale.default_roles,
            vec![PrincipalRole::Admin, PrincipalRole::Werka]
        );
    }

    #[test]
    fn capabilities_can_be_listed_by_role_and_resolved_by_code() {
        let werka = capabilities_for_role(PrincipalRole::Werka);

        assert!(werka.contains(&Capability::WerkaAccess));
        assert!(werka.contains(&Capability::GscaleCatalogRead));
        assert!(werka.contains(&Capability::RpsBatchManage));
        assert_eq!(
            capability_by_code("gscale.print").map(|item| item.capability),
            Some(Capability::GscalePrint)
        );
        assert!(capability_by_code("missing.capability").is_none());
    }

    #[test]
    fn admin_business_capabilities_are_named_for_real_workflows() {
        let admin = principal(PrincipalRole::Admin);

        for capability in [
            Capability::RoleCapabilityRead,
            Capability::RoleCapabilityManage,
            Capability::AdminSettingsRead,
            Capability::AdminSettingsManage,
            Capability::CatalogItemRead,
            Capability::CatalogItemCreate,
            Capability::CatalogItemGroupRead,
            Capability::CatalogItemGroupManage,
            Capability::CatalogItemBulkMove,
            Capability::SupplierDirectoryRead,
            Capability::SupplierDirectoryManage,
            Capability::SupplierItemAssign,
            Capability::SupplierCodeManage,
            Capability::CustomerDirectoryRead,
            Capability::CustomerDirectoryManage,
            Capability::CustomerItemAssign,
            Capability::CustomerCodeManage,
            Capability::AdminActivityRead,
            Capability::WerkaCodeManage,
        ] {
            assert!(has_capability(&admin, capability));
        }

        assert_eq!(
            capability_by_code("catalog.item.create").map(|item| item.capability),
            Some(Capability::CatalogItemCreate)
        );
        assert_eq!(
            capability_by_code("party.supplier.item.assign").map(|item| item.capability),
            Some(Capability::SupplierItemAssign)
        );
    }

    #[test]
    fn custom_role_definition_normalizes_ids_and_capabilities() {
        let role = normalize_custom_role(RoleDefinitionUpsert {
            id: " Scale_Operator ".to_string(),
            label: " Scale operator ".to_string(),
            base_role: PrincipalRole::Werka,
            capability_codes: vec![
                "gscale.print".to_string(),
                "gscale.catalog.read".to_string(),
                "gscale.print".to_string(),
            ],
        })
        .expect("role");

        assert_eq!(role.id, "scale_operator");
        assert_eq!(role.label, "Scale operator");
        assert_eq!(role.base_role, PrincipalRole::Werka);
        assert_eq!(
            role.capability_codes,
            vec![
                "gscale.catalog.read".to_string(),
                "gscale.print".to_string()
            ]
        );
        assert!(!role.system);
    }

    fn principal(role: PrincipalRole) -> Principal {
        Principal {
            role,
            display_name: String::new(),
            legal_name: String::new(),
            ref_: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }
}
