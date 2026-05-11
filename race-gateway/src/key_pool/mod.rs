use anyhow::{bail, ensure};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};

use crate::domain::{RaceKey, RaceKeyPool};
use crate::group::mask_key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPoolSnapshot {
    pub key_count: usize,
    pub masked_keys: Vec<String>,
}

pub trait KeyPoolSelector {
    fn select_key<'a>(
        &self,
        pool: &'a RaceKeyPool,
        request_seed: Option<u64>,
    ) -> anyhow::Result<&'a RaceKey>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RandomKeyPoolSelector;

impl KeyPoolSelector for RandomKeyPoolSelector {
    fn select_key<'a>(
        &self,
        pool: &'a RaceKeyPool,
        request_seed: Option<u64>,
    ) -> anyhow::Result<&'a RaceKey> {
        ensure!(pool.enabled, "key pool '{}' is disabled", pool.id);

        let enabled_keys = pool
            .keys
            .iter()
            .filter(|key| key.enabled && !key.secret.trim().is_empty())
            .collect::<Vec<_>>();
        if enabled_keys.is_empty() {
            bail!("key pool '{}' does not contain any enabled key", pool.id);
        }

        let selected = if let Some(seed) = request_seed {
            let mut rng = StdRng::seed_from_u64(seed);
            enabled_keys
                .choose(&mut rng)
                .copied()
                .expect("enabled_keys is not empty")
        } else {
            let index = rand::thread_rng().gen_range(0..enabled_keys.len());
            enabled_keys[index]
        };

        Ok(selected)
    }
}

pub fn snapshot(pool: &RaceKeyPool) -> KeyPoolSnapshot {
    let masked_keys = pool
        .keys
        .iter()
        .filter(|key| key.enabled && !key.secret.trim().is_empty())
        .map(|key| mask_key(&key.secret))
        .collect::<Vec<_>>();

    KeyPoolSnapshot {
        key_count: masked_keys.len(),
        masked_keys,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::{AuthStrategy, KeySelectionStrategy};

    fn pool() -> RaceKeyPool {
        RaceKeyPool {
            id: "pool-a".to_string(),
            display_name: "Pool A".to_string(),
            auth_strategy: AuthStrategy::Bearer,
            selection_strategy: KeySelectionStrategy::Random,
            enabled: true,
            keys: vec![
                RaceKey {
                    id: "key-a".to_string(),
                    key_pool_id: "pool-a".to_string(),
                    secret: "secret-a".to_string(),
                    enabled: false,
                    metadata: json!({}),
                },
                RaceKey {
                    id: "key-b".to_string(),
                    key_pool_id: "pool-a".to_string(),
                    secret: "secret-b".to_string(),
                    enabled: true,
                    metadata: json!({}),
                },
            ],
        }
    }

    #[test]
    fn selector_skips_disabled_keys() {
        let selector = RandomKeyPoolSelector;
        let pool = pool();
        let selected = selector.select_key(&pool, Some(42)).expect("select key");
        assert_eq!(selected.id, "key-b");
    }

    #[test]
    fn snapshot_masks_enabled_keys() {
        let snapshot = snapshot(&pool());
        assert_eq!(snapshot.key_count, 1);
        assert_eq!(snapshot.masked_keys, vec!["secret-b***"]);
    }
}
