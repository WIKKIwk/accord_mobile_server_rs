use std::sync::Arc;

use crate::core::werka::models::{DispatchRecord, WerkaHomeData, WerkaHomeSummary};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};

#[derive(Clone, Default)]
pub struct WerkaService {
    lookup: Option<Arc<dyn WerkaHomeLookup>>,
}

impl WerkaService {
    pub fn new() -> Self {
        Self { lookup: None }
    }

    #[allow(dead_code)]
    pub fn with_lookup(mut self, lookup: Arc<dyn WerkaHomeLookup>) -> Self {
        self.lookup = Some(lookup);
        self
    }

    pub async fn home(
        &self,
        pending_limit: usize,
    ) -> Result<Option<WerkaHomeData>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_home(pending_limit).await.map(Some)
    }

    pub async fn summary(&self) -> Result<Option<WerkaHomeSummary>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_summary().await.map(Some)
    }

    pub async fn pending(
        &self,
        limit: usize,
    ) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_pending(limit).await.map(Some)
    }

    pub async fn history(&self) -> Result<Option<Vec<DispatchRecord>>, WerkaPortError> {
        let Some(lookup) = &self.lookup else {
            return Ok(None);
        };

        lookup.werka_history().await.map(Some)
    }
}
