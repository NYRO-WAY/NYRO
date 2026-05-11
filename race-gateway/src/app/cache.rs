use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

use crate::domain::{RaceCandidate, RaceGroup, RaceKeyPool, RaceModelDescriptor, RaceSettings};
use crate::storage::RaceConfigStore;

#[derive(Debug, Clone)]
pub struct ConfigSnapshotCache {
    groups: Arc<RwLock<HashMap<String, RaceGroup>>>,
    models: Arc<RwLock<HashMap<String, RaceModelDescriptor>>>,
    model_ids_by_upstream_model: Arc<RwLock<HashMap<String, String>>>,
    key_pools: Arc<RwLock<HashMap<String, RaceKeyPool>>>,
    settings: Arc<RwLock<RaceSettings>>,
}

impl ConfigSnapshotCache {
    pub async fn load_from_store(store: &dyn RaceConfigStore) -> anyhow::Result<Self> {
        let mut groups = HashMap::new();
        for group in store.load_groups_full().await? {
            groups.insert(group.id.clone(), group);
        }

        let mut models = HashMap::new();
        for model in store.load_models_full().await? {
            models.insert(model.id.clone(), model);
        }
        let model_ids_by_upstream_model = models
            .values()
            .map(|model| (model.upstream_model.clone(), model.id.clone()))
            .collect();

        let mut key_pools = HashMap::new();
        for pool in store.load_key_pools_full().await? {
            key_pools.insert(pool.id.clone(), pool);
        }

        let settings = store.get_settings().await?;

        Ok(Self {
            groups: Arc::new(RwLock::new(groups)),
            models: Arc::new(RwLock::new(models)),
            model_ids_by_upstream_model: Arc::new(RwLock::new(model_ids_by_upstream_model)),
            key_pools: Arc::new(RwLock::new(key_pools)),
            settings: Arc::new(RwLock::new(settings)),
        })
    }

    pub async fn reload_from_store(&self, store: &dyn RaceConfigStore) -> anyhow::Result<()> {
        let fresh = Self::load_from_store(store).await?;

        *self
            .groups
            .write()
            .expect("config snapshot group lock poisoned") = fresh
            .groups
            .read()
            .expect("fresh config snapshot group lock poisoned")
            .clone();
        *self
            .models
            .write()
            .expect("config snapshot model lock poisoned") = fresh
            .models
            .read()
            .expect("fresh config snapshot model lock poisoned")
            .clone();
        *self
            .model_ids_by_upstream_model
            .write()
            .expect("config snapshot model upstream index lock poisoned") = fresh
            .model_ids_by_upstream_model
            .read()
            .expect("fresh config snapshot model upstream index lock poisoned")
            .clone();
        *self
            .key_pools
            .write()
            .expect("config snapshot key pool lock poisoned") = fresh
            .key_pools
            .read()
            .expect("fresh config snapshot key pool lock poisoned")
            .clone();
        *self
            .settings
            .write()
            .expect("config snapshot settings lock poisoned") = fresh
            .settings
            .read()
            .expect("fresh config snapshot settings lock poisoned")
            .clone();

        Ok(())
    }

    pub fn get_group(&self, group_id: &str) -> Option<RaceGroup> {
        self.groups
            .read()
            .expect("config snapshot group lock poisoned")
            .get(group_id)
            .cloned()
    }

    pub fn put_group(&self, group: RaceGroup) {
        self.groups
            .write()
            .expect("config snapshot group lock poisoned")
            .insert(group.id.clone(), group);
    }

    pub fn delete_group(&self, group_id: &str) {
        self.groups
            .write()
            .expect("config snapshot group lock poisoned")
            .remove(group_id);
    }

    pub fn list_groups(&self) -> Vec<RaceGroup> {
        self.groups
            .read()
            .expect("config snapshot group lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn get_model(&self, model_id: &str) -> Option<RaceModelDescriptor> {
        self.models
            .read()
            .expect("config snapshot model lock poisoned")
            .get(model_id)
            .cloned()
    }

    pub fn put_model(&self, model: RaceModelDescriptor) {
        let previous = self
            .models
            .write()
            .expect("config snapshot model lock poisoned")
            .insert(model.id.clone(), model.clone());
        let mut upstream_index = self
            .model_ids_by_upstream_model
            .write()
            .expect("config snapshot model upstream index lock poisoned");
        if let Some(previous) = previous {
            upstream_index.remove(&previous.upstream_model);
        }
        upstream_index.insert(model.upstream_model.clone(), model.id.clone());
    }

    pub fn delete_model(&self, model_id: &str) {
        let removed = self
            .models
            .write()
            .expect("config snapshot model lock poisoned")
            .remove(model_id);
        if let Some(removed) = removed {
            self.model_ids_by_upstream_model
                .write()
                .expect("config snapshot model upstream index lock poisoned")
                .remove(&removed.upstream_model);
        }
    }

    pub fn list_models(&self) -> Vec<RaceModelDescriptor> {
        self.models
            .read()
            .expect("config snapshot model lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn models_for_candidates(
        &self,
        candidates: &[RaceCandidate],
    ) -> BTreeMap<String, RaceModelDescriptor> {
        let models = self
            .models
            .read()
            .expect("config snapshot model lock poisoned");
        let upstream_index = self
            .model_ids_by_upstream_model
            .read()
            .expect("config snapshot model upstream index lock poisoned");
        let mut resolved = BTreeMap::new();

        for candidate in candidates {
            if let Some(model_id) = candidate.model_id.as_deref() {
                if let Some(model) = models.get(model_id) {
                    resolved.insert(model.id.clone(), model.clone());
                }
                continue;
            }

            let upstream_model = candidate.upstream_model.trim();
            if upstream_model.is_empty() {
                continue;
            }

            let Some(model_id) = upstream_index.get(upstream_model) else {
                continue;
            };
            if let Some(model) = models.get(model_id) {
                resolved.insert(model.id.clone(), model.clone());
            }
        }

        resolved
    }

    pub fn models_for_groups(&self, groups: &[RaceGroup]) -> BTreeMap<String, RaceModelDescriptor> {
        let mut candidates = Vec::new();
        for group in groups {
            candidates.extend(group.candidates.iter().cloned());
        }
        self.models_for_candidates(&candidates)
    }

    pub fn get_key_pool(&self, key_pool_id: &str) -> Option<RaceKeyPool> {
        self.key_pools
            .read()
            .expect("config snapshot key pool lock poisoned")
            .get(key_pool_id)
            .cloned()
    }

    pub fn put_key_pool(&self, key_pool: RaceKeyPool) {
        self.key_pools
            .write()
            .expect("config snapshot key pool lock poisoned")
            .insert(key_pool.id.clone(), key_pool);
    }

    pub fn delete_key_pool(&self, key_pool_id: &str) {
        self.key_pools
            .write()
            .expect("config snapshot key pool lock poisoned")
            .remove(key_pool_id);
    }

    pub fn list_key_pools(&self) -> Vec<RaceKeyPool> {
        self.key_pools
            .read()
            .expect("config snapshot key pool lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn get_settings(&self) -> RaceSettings {
        self.settings
            .read()
            .expect("config snapshot settings lock poisoned")
            .clone()
    }

    pub fn put_settings(&self, settings: RaceSettings) {
        *self
            .settings
            .write()
            .expect("config snapshot settings lock poisoned") = settings;
    }
}
