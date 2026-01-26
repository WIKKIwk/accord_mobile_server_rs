use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::core::profile::ports::{ProfilePrefs, ProfileStoreError, ProfileStorePort};
use crate::store::json_file::{read_map, write_pretty};

#[derive(Clone)]
pub struct ProfileStore {
    path: PathBuf,
    state: Arc<Mutex<ProfileStoreState>>,
}

#[derive(Default)]
struct ProfileStoreState {
    loaded: bool,
    cache: HashMap<String, ProfilePrefs>,
}

impl ProfileStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Arc::new(Mutex::new(ProfileStoreState::default())),
        }
    }
}

#[async_trait]
impl ProfileStorePort for ProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        Ok(state.cache.get(key).cloned().unwrap_or_default())
    }

    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        state.cache.insert(key.to_string(), prefs);
        write_pretty(&self.path, &state.cache)
            .await
            .map_err(|_| ProfileStoreError::StoreFailed)?;
        Ok(())
    }
}

async fn load_if_needed(
    path: &PathBuf,
    state: &mut ProfileStoreState,
) -> Result<(), ProfileStoreError> {
    if state.loaded {
        return Ok(());
    }
    let data = read_map::<ProfilePrefs>(path)
        .await
        .map_err(|_| ProfileStoreError::StoreFailed)?;
    state.cache = data.into_iter().collect();
    state.loaded = true;
    Ok(())
}
