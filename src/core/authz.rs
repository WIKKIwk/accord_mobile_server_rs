use serde::Serialize;

use crate::core::auth::models::{Principal, PrincipalRole};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Capability {
    AdminAccess,
    WerkaAccess,
    SupplierAccess,
    CustomerAccess,
    PushTokenManage,
    SupplierAvatarManage,
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

pub fn capabilities_for_role(role: PrincipalRole) -> Vec<Capability> {
    capability_catalog()
        .iter()
        .filter(|definition| definition.default_roles.contains(&role))
        .map(|definition| definition.capability)
        .collect()
}

pub fn has_capability(principal: &Principal, capability: Capability) -> bool {
    capability_catalog()
        .iter()
        .find(|definition| definition.capability == capability)
        .map(|definition| definition.default_roles.contains(&principal.role))
        .unwrap_or(false)
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
