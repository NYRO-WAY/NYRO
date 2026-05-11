use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use crate::domain::{
    ProtocolFamily, RaceCandidate, RaceGroup, RaceModelDescriptor,
    candidate_effective_upstream_model, resolve_model_for_candidate,
};
use crate::group::{CandidateWeightSnapshot, RaceStats, RaceStatsSnapshot, WeightTracker};

#[derive(Debug, Clone)]
pub struct RuntimeRegistry {
    groups: Arc<Mutex<HashMap<String, Arc<GroupRuntimeHandle>>>>,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self {
            groups: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn ensure_group(&self, group: &RaceGroup) -> Arc<GroupRuntimeHandle> {
        let handle = {
            let mut groups = self.groups.lock().expect("runtime registry lock poisoned");
            groups
                .entry(group.id.clone())
                .or_insert_with(|| Arc::new(GroupRuntimeHandle::new(group)))
                .clone()
        };
        handle.reconfigure(group);
        handle
    }

    pub fn get_group(&self, group_id: &str) -> Option<Arc<GroupRuntimeHandle>> {
        self.groups
            .lock()
            .expect("runtime registry lock poisoned")
            .get(group_id)
            .cloned()
    }

    pub fn delete_group(&self, group_id: &str) {
        self.groups
            .lock()
            .expect("runtime registry lock poisoned")
            .remove(group_id);
    }
}

#[derive(Debug)]
pub struct GroupRuntimeHandle {
    weight_tracker: Mutex<WeightTracker>,
    stats: Mutex<RaceStats>,
}

impl GroupRuntimeHandle {
    fn new(group: &RaceGroup) -> Self {
        Self {
            weight_tracker: Mutex::new(WeightTracker::new(
                &group.candidates,
                group.penalty_rate,
                group.recovery_rate,
            )),
            stats: Mutex::new(RaceStats::default()),
        }
    }

    pub fn reconfigure(&self, group: &RaceGroup) {
        self.weight_tracker
            .lock()
            .expect("weight tracker lock poisoned")
            .reconfigure(&group.candidates, group.penalty_rate, group.recovery_rate);
    }

    pub fn snapshot_weights(&self) -> BTreeMap<String, CandidateWeightSnapshot> {
        self.weight_tracker
            .lock()
            .expect("weight tracker lock poisoned")
            .snapshot()
    }

    pub fn with_weight_tracker<R>(&self, f: impl FnOnce(&mut WeightTracker) -> R) -> R {
        let mut tracker = self
            .weight_tracker
            .lock()
            .expect("weight tracker lock poisoned");
        f(&mut tracker)
    }

    pub fn with_stats<R>(&self, f: impl FnOnce(&mut RaceStats) -> R) -> R {
        let mut stats = self.stats.lock().expect("stats lock poisoned");
        f(&mut stats)
    }

    pub fn stats_snapshot(&self) -> RaceStatsSnapshot {
        self.stats.lock().expect("stats lock poisoned").snapshot()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CandidateRuntimeSnapshot {
    pub candidate_id: String,
    pub candidate_name: String,
    pub upstream_model: String,
    pub enabled: bool,
    pub eligible_protocol_families: Vec<ProtocolFamily>,
    pub response_protection_timeout_ms: u64,
    pub initial_weight: f64,
    pub effective_weight: f64,
    pub weight_deviation: f64,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupRuntimeSnapshot {
    pub group_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub eligible_candidate_counts_by_protocol: BTreeMap<String, usize>,
    pub effective_weights: BTreeMap<String, CandidateWeightSnapshot>,
    pub candidate_statuses: Vec<CandidateRuntimeSnapshot>,
    pub race_stats: RaceStatsSnapshot,
}

#[derive(Debug, Clone)]
struct CandidateResolvedView {
    effective_upstream_model: String,
    eligible_protocol_families: Vec<ProtocolFamily>,
}

pub fn build_group_runtime_snapshot(
    group: &RaceGroup,
    models: &BTreeMap<String, RaceModelDescriptor>,
    handle: &GroupRuntimeHandle,
) -> GroupRuntimeSnapshot {
    let effective_weights = handle.snapshot_weights();
    let race_stats = handle.stats_snapshot();
    let resolved_views = build_candidate_resolved_views(group, models);
    let candidate_statuses = group
        .candidates
        .iter()
        .map(|candidate| {
            build_candidate_runtime_snapshot(candidate, &resolved_views, &effective_weights)
        })
        .collect::<Vec<_>>();

    GroupRuntimeSnapshot {
        group_id: group.id.clone(),
        display_name: group.display_name.clone(),
        enabled: group.enabled,
        eligible_candidate_counts_by_protocol: eligible_counts(group, &resolved_views),
        effective_weights,
        candidate_statuses,
        race_stats,
    }
}

fn build_candidate_runtime_snapshot(
    candidate: &RaceCandidate,
    resolved_views: &HashMap<String, CandidateResolvedView>,
    weights: &BTreeMap<String, CandidateWeightSnapshot>,
) -> CandidateRuntimeSnapshot {
    let weight = weights
        .get(&candidate.name)
        .cloned()
        .unwrap_or(CandidateWeightSnapshot {
            initial_weight: candidate.initial_weight,
            effective_weight: candidate.initial_weight,
            weight_deviation: 0.0,
            status: "normal".to_string(),
        });
    let resolved = resolved_views
        .get(&candidate.id)
        .cloned()
        .unwrap_or_else(|| CandidateResolvedView {
            effective_upstream_model: candidate.upstream_model.clone(),
            eligible_protocol_families: Vec::new(),
        });

    CandidateRuntimeSnapshot {
        candidate_id: candidate.id.clone(),
        candidate_name: candidate.name.clone(),
        upstream_model: resolved.effective_upstream_model,
        enabled: candidate.enabled,
        eligible_protocol_families: resolved.eligible_protocol_families,
        response_protection_timeout_ms: candidate.response_protection_timeout_ms,
        initial_weight: weight.initial_weight,
        effective_weight: weight.effective_weight,
        weight_deviation: weight.weight_deviation,
        status: weight.status,
    }
}

fn eligible_counts(
    group: &RaceGroup,
    resolved_views: &HashMap<String, CandidateResolvedView>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for protocol in [
        ProtocolFamily::OpenAi,
        ProtocolFamily::Anthropic,
        ProtocolFamily::Google,
    ] {
        let count = group
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.enabled
                    && resolved_views
                        .get(&candidate.id)
                        .is_some_and(|view| view.eligible_protocol_families.contains(&protocol))
            })
            .count();
        counts.insert(protocol_label(protocol).to_string(), count);
    }
    counts
}

fn build_candidate_resolved_views(
    group: &RaceGroup,
    models: &BTreeMap<String, RaceModelDescriptor>,
) -> HashMap<String, CandidateResolvedView> {
    group
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.id.clone(),
                CandidateResolvedView {
                    effective_upstream_model: candidate_effective_upstream_model(
                        candidate,
                        resolve_model_for_candidate(candidate, models)
                            .ok()
                            .flatten()
                            .as_ref(),
                    ),
                    eligible_protocol_families: eligible_protocols(candidate, models),
                },
            )
        })
        .collect()
}

fn eligible_protocols(
    candidate: &RaceCandidate,
    models: &BTreeMap<String, RaceModelDescriptor>,
) -> Vec<ProtocolFamily> {
    let mut protocols = BTreeSet::new();
    for endpoint in candidate
        .inline_endpoint_overrides
        .iter()
        .filter(|endpoint| endpoint.enabled)
    {
        protocols.insert(endpoint.protocol_family);
    }
    if let Some(model) = resolve_model_for_candidate(candidate, models)
        .ok()
        .flatten()
    {
        for endpoint in model.endpoints.iter().filter(|endpoint| endpoint.enabled) {
            protocols.insert(endpoint.protocol_family);
        }
    }
    protocols.into_iter().collect()
}

fn protocol_label(protocol: ProtocolFamily) -> &'static str {
    match protocol {
        ProtocolFamily::OpenAi => "openai",
        ProtocolFamily::Anthropic => "anthropic",
        ProtocolFamily::Google => "google",
    }
}
