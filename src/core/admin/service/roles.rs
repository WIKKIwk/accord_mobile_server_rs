use super::*;

impl AdminService {
    pub fn with_role_store(mut self, role_store: Arc<dyn RoleDefinitionStorePort>) -> Self {
        self.role_store = role_store;
        self
    }

    pub async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, AdminPortError> {
        self.all_role_definitions().await
    }

    async fn all_role_definitions(&self) -> Result<Vec<RoleDefinition>, AdminPortError> {
        let mut roles = system_role_definitions();
        roles.extend(
            self.role_store
                .role_definitions()
                .await
                .map_err(|_| AdminPortError::LookupFailed)?,
        );
        roles.sort_by(|left, right| {
            left.system
                .cmp(&right.system)
                .reverse()
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(roles)
    }

    pub async fn role_assignments(&self) -> Result<Vec<RoleAssignment>, AdminPortError> {
        self.role_store
            .role_assignments()
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }

    pub async fn upsert_role_definition(
        &self,
        input: RoleDefinitionUpsert,
    ) -> Result<RoleDefinition, AdminPortError> {
        let role = normalize_custom_role(input)
            .map_err(|error| AdminPortError::InvalidInput(error.to_string()))?;
        self.role_store
            .put_role_definition(role.clone())
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(role)
    }

    pub async fn upsert_role_assignment(
        &self,
        input: RoleAssignmentUpsert,
    ) -> Result<RoleAssignment, AdminPortError> {
        let roles = self.all_role_definitions().await?;
        let assignment = normalize_role_assignment(input, &roles)
            .map_err(|error| AdminPortError::InvalidInput(error.to_string()))?;
        self.role_store
            .put_role_assignment(assignment.clone())
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(assignment)
    }

    pub async fn principal_has_capability(
        &self,
        principal: &Principal,
        capability: Capability,
    ) -> bool {
        match self.principal_assigned_role(principal).await {
            Ok(Some(role)) => capability_code(capability)
                .map(|code| role.capability_codes.iter().any(|item| item == code))
                .unwrap_or(false),
            Ok(None) => has_capability(principal, capability),
            Err(_) => false,
        }
    }

    async fn principal_assigned_role(
        &self,
        principal: &Principal,
    ) -> Result<Option<RoleDefinition>, AdminPortError> {
        let key = role_assignment_key(&principal.role, &principal.ref_);
        let Some(assignment) = self
            .role_assignments()
            .await?
            .into_iter()
            .find(|assignment| {
                role_assignment_key(&assignment.principal_role, &assignment.principal_ref) == key
            })
        else {
            return Ok(None);
        };
        self.all_role_definitions()
            .await?
            .into_iter()
            .find(|role| role.id == assignment.role_id)
            .map(Some)
            .ok_or(AdminPortError::LookupFailed)
    }
}
