use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const DEFAULT_FAILURE_THRESHOLD: u32 = 3;
const DEFAULT_RECOVERY_SECS: u64 = 30;

pub struct HealthRegistry {
    states: RwLock<HashMap<String, TargetHealth>>,
    failure_threshold: u32,
    recovery_after: Duration,
}

struct TargetHealth {
    consecutive_failures: u32,
    last_failure_at: Option<Instant>,
}

impl HealthRegistry {
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            recovery_after: Duration::from_secs(DEFAULT_RECOVERY_SECS),
        }
    }

    pub fn is_healthy(&self, target_key: &str) -> bool {
        let states = self.states.read().unwrap();
        match states.get(target_key) {
            None => true,
            Some(state) => {
                if state.consecutive_failures < self.failure_threshold {
                    return true;
                }
                state
                    .last_failure_at
                    .map(|failed_at| failed_at.elapsed() >= self.recovery_after)
                    .unwrap_or(true)
            }
        }
    }

    pub fn record_success(&self, target_key: &str) {
        let mut states = self.states.write().unwrap();
        if let Some(state) = states.get_mut(target_key) {
            state.consecutive_failures = 0;
        }
    }

    pub fn record_failure(&self, target_key: &str) {
        let mut states = self.states.write().unwrap();
        let entry = states
            .entry(target_key.to_string())
            .or_insert(TargetHealth {
                consecutive_failures: 0,
                last_failure_at: None,
            });
        entry.consecutive_failures += 1;
        entry.last_failure_at = Some(Instant::now());
    }
}
