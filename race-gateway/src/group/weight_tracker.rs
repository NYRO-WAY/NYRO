use std::collections::BTreeMap;
use std::time::Instant;

use crate::domain::RaceCandidate;

use super::CandidateWeightSnapshot;

#[derive(Debug, Clone)]
pub struct WeightTracker {
    original: BTreeMap<String, f64>,
    effective: BTreeMap<String, f64>,
    last_update: Instant,
    penalty_rate: f64,
    recovery_rate: f64,
}

impl WeightTracker {
    pub fn new(candidates: &[RaceCandidate], penalty_rate: f64, recovery_rate: f64) -> Self {
        let original = candidates
            .iter()
            .map(|candidate| (candidate.name.clone(), candidate.initial_weight))
            .collect::<BTreeMap<_, _>>();

        Self {
            effective: original.clone(),
            original,
            last_update: Instant::now(),
            penalty_rate,
            recovery_rate,
        }
    }

    pub fn candidate_names(&self) -> Vec<String> {
        self.original.keys().cloned().collect()
    }

    pub fn reconfigure(
        &mut self,
        candidates: &[RaceCandidate],
        penalty_rate: f64,
        recovery_rate: f64,
    ) {
        self.tick();
        let old_effective = self.effective.clone();
        self.original = candidates
            .iter()
            .map(|candidate| (candidate.name.clone(), candidate.initial_weight))
            .collect();
        self.effective = self
            .original
            .iter()
            .map(|(name, original)| {
                let effective = old_effective.get(name).copied().unwrap_or(*original);
                (name.clone(), effective.min(*original))
            })
            .collect();
        self.penalty_rate = penalty_rate;
        self.recovery_rate = recovery_rate;
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_update);
        self.last_update = now;

        if self.recovery_rate <= 0.0 {
            return;
        }

        for (name, original) in &self.original {
            if let Some(effective) = self.effective.get_mut(name)
                && *effective < *original
            {
                *effective =
                    (*effective + (self.recovery_rate * elapsed.as_secs_f64())).min(*original);
            }
        }
    }

    pub fn apply_penalty(&mut self, penalized_names: &[String]) {
        self.tick();
        for name in penalized_names {
            if let Some(weight) = self.effective.get_mut(name) {
                *weight = (*weight - self.penalty_rate).max(0.0);
            }
        }
    }

    pub fn effective_weight(&mut self, name: &str) -> f64 {
        self.tick();
        self.effective
            .get(name)
            .copied()
            .or_else(|| self.original.get(name).copied())
            .unwrap_or(0.0)
    }

    pub fn sorted_candidates(&mut self, candidates: &[RaceCandidate]) -> Vec<RaceCandidate> {
        self.tick();
        let mut items = candidates.to_vec();
        items.sort_by(|left, right| {
            self.effective
                .get(&right.name)
                .unwrap_or(&0.0)
                .partial_cmp(self.effective.get(&left.name).unwrap_or(&0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        items
    }

    pub fn snapshot(&mut self) -> BTreeMap<String, CandidateWeightSnapshot> {
        self.tick();
        self.original
            .iter()
            .map(|(name, original)| {
                let effective = self.effective.get(name).copied().unwrap_or(*original);
                let deviation = effective - original;
                let status = if deviation < 0.0 {
                    if deviation > -(self.penalty_rate * 2.0) {
                        "recovering"
                    } else {
                        "penalized"
                    }
                } else {
                    "normal"
                };
                (
                    name.clone(),
                    CandidateWeightSnapshot {
                        initial_weight: *original,
                        effective_weight: effective,
                        weight_deviation: deviation,
                        status: status.to_string(),
                    },
                )
            })
            .collect()
    }

    pub fn reset(&mut self) {
        self.effective = self.original.clone();
        self.last_update = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use serde_json::json;

    use super::*;

    fn candidate(name: &str, initial_weight: f64) -> RaceCandidate {
        RaceCandidate {
            id: name.to_string(),
            group_id: "g".to_string(),
            name: name.to_string(),
            model_id: None,
            upstream_model: format!("vendor/{name}"),
            inline_endpoint_overrides: vec![],
            initial_weight,
            response_protection_timeout_ms: 3_000,
            enabled: true,
            metadata: json!({}),
        }
    }

    #[test]
    fn tracker_recovers_lazily() {
        let mut tracker = WeightTracker::new(&[candidate("a", 100.0)], 20.0, 200.0);
        tracker.apply_penalty(&["a".to_string()]);
        let after_penalty = tracker.effective_weight("a");
        assert!(after_penalty < 100.0);

        thread::sleep(Duration::from_millis(20));
        let recovered = tracker.effective_weight("a");
        assert!(recovered > after_penalty);
        assert!(recovered <= 100.0);
    }

    #[test]
    fn tracker_preserves_order_for_equal_weights() {
        let candidates = vec![candidate("a", 100.0), candidate("b", 100.0)];
        let mut tracker = WeightTracker::new(&candidates, 5.0, 0.0);
        let sorted = tracker.sorted_candidates(&candidates);
        assert_eq!(sorted[0].name, "a");
        assert_eq!(sorted[1].name, "b");
    }
}
