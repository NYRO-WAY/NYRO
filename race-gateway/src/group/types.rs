use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::ProtocolFamily;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateWeightSnapshot {
    pub initial_weight: f64,
    pub effective_weight: f64,
    pub weight_deviation: f64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceRecord {
    pub id: u64,
    pub timestamp: DateTime<Utc>,
    pub group_id: String,
    pub protocol: ProtocolFamily,
    pub winner: Option<String>,
    pub duration_ms: u64,
    pub buffer_events: u64,
    pub participants: Vec<String>,
    pub first_content_times_ms: BTreeMap<String, u64>,
    pub penalty_applied: bool,
    #[serde(default)]
    pub errors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolStatsSnapshot {
    pub total: u64,
    pub wins: BTreeMap<String, u64>,
    pub avg_race_duration_ms: f64,
    pub avg_buffer_events: f64,
    pub all_failed_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceStatsSnapshot {
    pub total_races: u64,
    pub winner_distribution: BTreeMap<String, u64>,
    pub protocol_distribution: BTreeMap<String, u64>,
    pub by_protocol: BTreeMap<String, ProtocolStatsSnapshot>,
    pub avg_race_duration_ms: f64,
    pub avg_buffer_events: f64,
    pub recent_races: Vec<RaceRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateDiagnosticsPayload {
    pub name: String,
    pub upstream_model: String,
    pub key: String,
    pub delay_s: f64,
    pub launch_offset_s: Option<f64>,
    pub first_content_offset_s: Option<f64>,
    pub initial_weight: f64,
    pub effective_weight: f64,
    pub weight_deviation: f64,
    pub status: String,
    pub failed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RaceDiagnosticsHeaderPayload {
    pub group: String,
    pub protocol: ProtocolFamily,
    pub winner: Option<String>,
    pub penalty_applied: bool,
    pub penalized_candidates: Vec<String>,
    pub duration_ms: Option<u64>,
    pub all_failed: bool,
    pub candidates: Vec<CandidateDiagnosticsPayload>,
}
