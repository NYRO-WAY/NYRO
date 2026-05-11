use std::time::Duration;

use crate::domain::RaceCandidate;

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledCandidate {
    pub candidate: RaceCandidate,
    pub relative_delay: Duration,
}

pub fn compute_schedule(
    candidates: &[RaceCandidate],
    fallback_ratio: f64,
    decay_factor: f64,
) -> Vec<ScheduledCandidate> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut schedule = Vec::with_capacity(candidates.len());
    schedule.push(ScheduledCandidate {
        candidate: candidates[0].clone(),
        relative_delay: Duration::ZERO,
    });

    for index in 1..candidates.len() {
        let previous = &candidates[index - 1];
        let delay_ms = previous.response_protection_timeout_ms as f64
            * fallback_ratio
            * decay_factor.powi((index - 1) as i32);

        schedule.push(ScheduledCandidate {
            candidate: candidates[index].clone(),
            relative_delay: duration_from_millis_f64(delay_ms),
        });
    }

    schedule
}

fn duration_from_millis_f64(value: f64) -> Duration {
    if !value.is_finite() || value <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(value / 1000.0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn candidate(name: &str, timeout_ms: u64) -> RaceCandidate {
        RaceCandidate {
            id: name.to_string(),
            group_id: "g".to_string(),
            name: name.to_string(),
            model_id: None,
            upstream_model: format!("vendor/{name}"),
            inline_endpoint_overrides: vec![],
            initial_weight: 100.0,
            response_protection_timeout_ms: timeout_ms,
            enabled: true,
            metadata: json!({}),
        }
    }

    #[test]
    fn compute_schedule_uses_relative_delay() {
        let items = vec![
            candidate("a", 5_000),
            candidate("b", 4_000),
            candidate("c", 3_000),
        ];
        let schedule = compute_schedule(&items, 0.5, 0.8);

        assert_eq!(schedule.len(), 3);
        assert_eq!(schedule[0].relative_delay, Duration::ZERO);
        assert_eq!(schedule[1].relative_delay, Duration::from_millis(2_500));
        assert_eq!(schedule[2].relative_delay, Duration::from_millis(1_600));
    }
}
