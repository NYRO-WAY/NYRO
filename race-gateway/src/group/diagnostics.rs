use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

use super::{CandidateDiagnosticsPayload, RaceDiagnosticsHeaderPayload};

pub const RACE_DIAGNOSTICS_HEADER: &str = "x-nyro-race-diagnostics";
pub const MAX_DIAGNOSTIC_ERROR_CHARS: usize = 240;

#[derive(Debug, Clone, Default)]
pub struct RaceDiagnosticsSink {
    state: Arc<RaceDiagnosticsSinkState>,
}

#[derive(Debug, Default)]
struct RaceDiagnosticsSinkState {
    payload: Mutex<Option<RaceDiagnosticsHeaderPayload>>,
    notify: Notify,
}

impl RaceDiagnosticsSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn set(&self, payload: RaceDiagnosticsHeaderPayload) {
        let mut guard = self.state.payload.lock().await;
        if guard.is_none() {
            *guard = Some(payload);
            self.state.notify.notify_waiters();
        }
    }

    pub async fn wait(&self) -> RaceDiagnosticsHeaderPayload {
        loop {
            if let Some(payload) = self.state.payload.lock().await.clone() {
                return payload;
            }
            self.state.notify.notified().await;
        }
    }
}

impl CandidateDiagnosticsPayload {
    pub fn masked(mut self) -> Self {
        self.key = mask_key(&self.key);
        self.error = truncate_error(self.error);
        self.delay_s = round(self.delay_s);
        self.launch_offset_s = self.launch_offset_s.map(round);
        self.first_content_offset_s = self.first_content_offset_s.map(round);
        self.initial_weight = round(self.initial_weight);
        self.effective_weight = round(self.effective_weight);
        self.weight_deviation = round(self.weight_deviation);
        self
    }
}

impl RaceDiagnosticsHeaderPayload {
    pub fn to_header_value(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    pub fn masked(&self) -> Self {
        Self {
            group: self.group.clone(),
            protocol: self.protocol,
            winner: self.winner.clone(),
            penalty_applied: self.penalty_applied,
            penalized_candidates: self.penalized_candidates.clone(),
            duration_ms: self.duration_ms,
            all_failed: self.all_failed,
            candidates: self
                .candidates
                .iter()
                .cloned()
                .map(CandidateDiagnosticsPayload::masked)
                .collect(),
        }
    }
}

pub fn mask_key(key: &str) -> String {
    let stripped = key.trim();
    if stripped.is_empty() {
        return String::new();
    }
    let prefix_len = stripped.len().min(8);
    format!("{}***", &stripped[..prefix_len])
}

pub fn truncate_error(error: Option<String>) -> Option<String> {
    error.map(|value| {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.len() <= MAX_DIAGNOSTIC_ERROR_CHARS {
            normalized
        } else {
            format!("{}...", &normalized[..MAX_DIAGNOSTIC_ERROR_CHARS - 3])
        }
    })
}

fn round(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use crate::domain::ProtocolFamily;

    use super::*;

    #[tokio::test]
    async fn diagnostics_sink_is_one_shot() {
        let sink = RaceDiagnosticsSink::new();
        let payload = RaceDiagnosticsHeaderPayload {
            group: "g".to_string(),
            protocol: ProtocolFamily::OpenAi,
            winner: Some("a".to_string()),
            penalty_applied: false,
            penalized_candidates: vec![],
            duration_ms: Some(10),
            all_failed: false,
            candidates: vec![],
        };
        sink.set(payload.clone()).await;
        sink.set(RaceDiagnosticsHeaderPayload {
            winner: Some("b".to_string()),
            ..payload.clone()
        })
        .await;

        let waited = sink.wait().await;
        assert_eq!(waited.winner.as_deref(), Some("a"));
    }

    #[test]
    fn masking_and_truncation_are_applied() {
        let candidate = CandidateDiagnosticsPayload {
            name: "a".to_string(),
            upstream_model: "vendor/a".to_string(),
            key: "sk-1234567890".to_string(),
            delay_s: 1.23456789,
            launch_offset_s: Some(0.12345678),
            first_content_offset_s: Some(0.23456789),
            initial_weight: 100.0,
            effective_weight: 95.12345678,
            weight_deviation: -4.8765432,
            status: "recovering".to_string(),
            failed: true,
            error: Some("x ".repeat(200)),
        }
        .masked();

        assert_eq!(candidate.key, "sk-12345***");
        assert!(candidate.error.as_deref().expect("error").len() <= MAX_DIAGNOSTIC_ERROR_CHARS);
    }
}
