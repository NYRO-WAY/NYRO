use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use anyhow::Context;
use async_stream::stream;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::adapters::RaceProtocolAdapter;
use crate::domain::{RaceCandidate, RaceGroup, RaceKey, RaceTargetEndpoint};
use crate::group::{
    CandidateDiagnosticsPayload, CandidateWeightSnapshot, RaceDiagnosticsHeaderPayload,
};
use crate::observability::{ActiveRaceGuard, Observability};
use crate::runtime::GroupRuntimeHandle;

use super::{ScheduledCandidate, compute_schedule};

pub const MAX_BUFFER_EVENTS: usize = 100_000;
pub const MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;
pub const BUFFER_BACKPRESSURE_TIMEOUT_MS: u64 = 100;

#[derive(Debug, Clone, Copy)]
pub struct RaceExecutionSettings {
    pub max_buffer_events: usize,
    pub max_buffer_bytes: usize,
    pub buffer_backpressure_timeout_ms: u64,
}

impl Default for RaceExecutionSettings {
    fn default() -> Self {
        Self {
            max_buffer_events: MAX_BUFFER_EVENTS,
            max_buffer_bytes: MAX_BUFFER_BYTES,
            buffer_backpressure_timeout_ms: BUFFER_BACKPRESSURE_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RaceParticipant {
    pub candidate: RaceCandidate,
    pub endpoint: RaceTargetEndpoint,
    pub selected_key: RaceKey,
    pub masked_key: String,
}

#[derive(Debug, Clone)]
pub struct CandidateState {
    pub candidate: RaceCandidate,
    pub launched_at: Option<Instant>,
    pub first_content_at: Option<Instant>,
    pub winner_selected: bool,
    pub failed: bool,
    pub ended: bool,
    pub error: Option<String>,
    pub buffered_count: usize,
    pub buffered_bytes: usize,
    pub api_key_masked: String,
    pub relative_delay: Duration,
    pub weight_snapshot: CandidateWeightSnapshot,
    pub protocol_flags: ProtocolFlags,
    pub protocol_open_blocks: Vec<usize>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProtocolFlags {
    pub emitted_done: bool,
    pub emitted_finish: bool,
    pub emitted_message_delta: bool,
    pub emitted_message_stop: bool,
}

impl CandidateState {
    fn new(
        candidate: RaceCandidate,
        api_key_masked: String,
        relative_delay: Duration,
        weight_snapshot: CandidateWeightSnapshot,
    ) -> Self {
        Self {
            candidate,
            launched_at: None,
            first_content_at: None,
            winner_selected: false,
            failed: false,
            ended: false,
            error: None,
            buffered_count: 0,
            buffered_bytes: 0,
            api_key_masked,
            relative_delay,
            weight_snapshot,
            protocol_flags: ProtocolFlags::default(),
            protocol_open_blocks: Vec::new(),
        }
    }
}

pub struct RaceStreamExecution {
    pub content_type: String,
    pub diagnostics_header_value: Option<String>,
    pub stream: Pin<Box<dyn Stream<Item = Bytes> + Send>>,
}

#[async_trait]
pub trait CandidateStreamFactory: Send + Sync {
    async fn open_stream(
        &self,
        participant: RaceParticipant,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>>;
}

struct CandidateQueue {
    sender: mpsc::Sender<Bytes>,
    receiver: Option<mpsc::Receiver<Bytes>>,
}

pub struct RaceCore {
    group: RaceGroup,
    participants: Vec<RaceParticipant>,
    runtime: Arc<GroupRuntimeHandle>,
    adapter: Arc<dyn RaceProtocolAdapter>,
    protocol_label: String,
    observability: Observability,
    diagnostics_enabled: bool,
    execution_settings: RaceExecutionSettings,
}

impl RaceCore {
    pub fn new(
        group: RaceGroup,
        participants: Vec<RaceParticipant>,
        runtime: Arc<GroupRuntimeHandle>,
        adapter: Arc<dyn RaceProtocolAdapter>,
        protocol_label: String,
        observability: Observability,
        diagnostics_enabled: bool,
        execution_settings: RaceExecutionSettings,
    ) -> Self {
        Self {
            group,
            participants,
            runtime,
            adapter,
            protocol_label,
            observability,
            diagnostics_enabled,
            execution_settings,
        }
    }

    pub async fn execute(
        self,
        stream_factory: Arc<dyn CandidateStreamFactory>,
        active_race_guard: ActiveRaceGuard,
    ) -> anyhow::Result<RaceStreamExecution> {
        let race_start = Instant::now();
        let (ranked_candidates, ranking_snapshot, state_map, mut queues) = self.prepare_states()?;
        let shared_buffered_events = Arc::new(AtomicUsize::new(0));
        let shared_buffered_bytes = Arc::new(AtomicUsize::new(0));
        let winner_signal = Arc::new(Notify::new());
        let senders = queues
            .iter()
            .map(|(name, queue)| (name.clone(), queue.sender.clone()))
            .collect::<HashMap<_, _>>();

        let mut tasks: HashMap<String, JoinHandle<()>> = HashMap::new();
        let adapter = self.adapter.clone();
        launch_candidates(
            ranked_candidates
                .iter()
                .map(|candidate| ScheduledCandidate {
                    candidate: candidate.participant.candidate.clone(),
                    relative_delay: candidate.relative_delay,
                })
                .collect(),
            self.participants.clone(),
            state_map.clone(),
            senders,
            stream_factory.clone(),
            adapter.clone(),
            shared_buffered_events,
            shared_buffered_bytes,
            winner_signal.clone(),
            self.execution_settings,
            &mut tasks,
        );

        let winner_name = find_winner(
            &self.group,
            &ranked_candidates,
            &state_map,
            winner_signal,
            race_start,
        )
        .await;

        if winner_name.is_none() {
            warn!(
                group_id = %self.group.id,
                protocol = %self.protocol_label,
                "all race candidates failed before producing usable content"
            );
            cancel_all_tasks(tasks).await;
            let errors = collect_errors(&state_map).await;
            self.record_stats(&state_map, race_start, None, 0, false, errors.clone())
                .await;
            self.observability.finish_race(
                &self.protocol_label,
                "all_failed",
                race_start.elapsed(),
            );
            drop(active_race_guard);
            let diagnostics_header_value = self
                .build_diagnostics(&state_map, race_start, None, false, Vec::new(), true)
                .await;
            let events = self.adapter.all_failed_events(&self.group.id, &errors);
            let stream = Box::pin(stream! {
                for event in events {
                    yield event;
                }
            }) as Pin<Box<dyn Stream<Item = Bytes> + Send>>;

            return Ok(RaceStreamExecution {
                content_type: self.adapter.response_content_type().to_string(),
                diagnostics_header_value,
                stream,
            });
        }

        let winner_name = winner_name.expect("winner exists");
        info!(
            group_id = %self.group.id,
            protocol = %self.protocol_label,
            winner = %winner_name,
            "race winner selected"
        );
        if let Some(state) = state_map.get(&winner_name) {
            state.lock().await.winner_selected = true;
        }

        let winner_task = tasks.remove(&winner_name).context("winner task missing")?;
        cancel_all_tasks(tasks).await;

        let winner_index = ranked_candidates
            .iter()
            .position(|candidate| candidate.participant.candidate.name == winner_name)
            .context("winner not found in ranked candidates")?;
        let winner_weight = ranking_snapshot
            .get(&winner_name)
            .copied()
            .unwrap_or_default();
        let penalized_names = ranked_candidates[..winner_index]
            .iter()
            .filter_map(|candidate| {
                let weight = ranking_snapshot
                    .get(&candidate.participant.candidate.name)
                    .copied()
                    .unwrap_or_default();
                (weight > winner_weight).then(|| candidate.participant.candidate.name.clone())
            })
            .collect::<Vec<_>>();

        self.runtime
            .with_weight_tracker(|tracker| tracker.apply_penalty(&penalized_names));

        let diagnostics_header_value = self
            .build_diagnostics(
                &state_map,
                race_start,
                Some(winner_name.clone()),
                !penalized_names.is_empty(),
                penalized_names.clone(),
                false,
            )
            .await;

        let winner_receiver = queues
            .remove(&winner_name)
            .and_then(|queue| queue.receiver)
            .context("winner queue missing receiver")?;
        let winner_state = state_map
            .get(&winner_name)
            .cloned()
            .context("winner state missing")?;
        let group_id = self.group.id.clone();
        let adapter = self.adapter.clone();
        let runtime = self.runtime.clone();
        let participants = state_map.clone();
        let winner_name_for_stats = winner_name.clone();
        let stream = spawn_winner_stream(
            winner_receiver,
            winner_task,
            winner_state,
            participants,
            runtime,
            adapter,
            group_id,
            winner_name_for_stats,
            self.observability.clone(),
            self.protocol_label.clone(),
            active_race_guard,
            !penalized_names.is_empty(),
            race_start,
        );

        Ok(RaceStreamExecution {
            content_type: self.adapter.response_content_type().to_string(),
            diagnostics_header_value,
            stream,
        })
    }

    fn prepare_states(
        &self,
    ) -> anyhow::Result<(
        Vec<PreparedParticipant>,
        BTreeMap<String, f64>,
        HashMap<String, Arc<Mutex<CandidateState>>>,
        HashMap<String, CandidateQueue>,
    )> {
        let weight_snapshot = self.runtime.snapshot_weights();

        let mut candidates = self.participants.clone();
        candidates.sort_by(|left, right| {
            let left_weight = weight_snapshot
                .get(&left.candidate.name)
                .map(|snapshot| snapshot.effective_weight)
                .unwrap_or(left.candidate.initial_weight);
            let right_weight = weight_snapshot
                .get(&right.candidate.name)
                .map(|snapshot| snapshot.effective_weight)
                .unwrap_or(right.candidate.initial_weight);
            right_weight
                .partial_cmp(&left_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let ranking_snapshot = candidates
            .iter()
            .map(|participant| {
                (
                    participant.candidate.name.clone(),
                    weight_snapshot
                        .get(&participant.candidate.name)
                        .map(|snapshot| snapshot.effective_weight)
                        .unwrap_or(participant.candidate.initial_weight),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let schedule = compute_schedule(
            &candidates
                .iter()
                .map(|participant| participant.candidate.clone())
                .collect::<Vec<_>>(),
            self.group.fallback_ratio,
            self.group.decay_factor,
        );

        let delays = schedule
            .into_iter()
            .map(|item| (item.candidate.name, item.relative_delay))
            .collect::<BTreeMap<_, _>>();

        let mut state_map = HashMap::new();
        let mut queues = HashMap::new();
        let mut prepared = Vec::new();

        for participant in candidates {
            let delay = delays
                .get(&participant.candidate.name)
                .copied()
                .unwrap_or(Duration::ZERO);
            let snapshot = weight_snapshot
                .get(&participant.candidate.name)
                .cloned()
                .unwrap_or(CandidateWeightSnapshot {
                    initial_weight: participant.candidate.initial_weight,
                    effective_weight: participant.candidate.initial_weight,
                    weight_deviation: 0.0,
                    status: "normal".to_string(),
                });
            let (sender, receiver) =
                mpsc::channel(self.execution_settings.max_buffer_events.max(1));
            state_map.insert(
                participant.candidate.name.clone(),
                Arc::new(Mutex::new(CandidateState::new(
                    participant.candidate.clone(),
                    participant.masked_key.clone(),
                    delay,
                    snapshot,
                ))),
            );
            queues.insert(
                participant.candidate.name.clone(),
                CandidateQueue {
                    sender,
                    receiver: Some(receiver),
                },
            );
            prepared.push(PreparedParticipant {
                participant,
                relative_delay: delay,
            });
        }

        Ok((prepared, ranking_snapshot, state_map, queues))
    }

    async fn build_diagnostics(
        &self,
        states: &HashMap<String, Arc<Mutex<CandidateState>>>,
        race_start: Instant,
        winner: Option<String>,
        penalty_applied: bool,
        penalized_candidates: Vec<String>,
        all_failed: bool,
    ) -> Option<String> {
        if !self.diagnostics_enabled {
            return None;
        }
        let mut candidates = Vec::new();
        for state in states.values() {
            let snapshot = state.lock().await.clone();
            candidates.push(CandidateDiagnosticsPayload {
                name: snapshot.candidate.name.clone(),
                upstream_model: snapshot.candidate.upstream_model.clone(),
                key: snapshot.api_key_masked,
                delay_s: snapshot.relative_delay.as_secs_f64(),
                launch_offset_s: snapshot
                    .launched_at
                    .map(|value| value.duration_since(race_start).as_secs_f64()),
                first_content_offset_s: snapshot
                    .first_content_at
                    .map(|value| value.duration_since(race_start).as_secs_f64()),
                initial_weight: snapshot.weight_snapshot.initial_weight,
                effective_weight: snapshot.weight_snapshot.effective_weight,
                weight_deviation: snapshot.weight_snapshot.weight_deviation,
                status: snapshot.weight_snapshot.status,
                failed: snapshot.failed,
                error: snapshot.error,
            });
        }

        RaceDiagnosticsHeaderPayload {
            group: self.group.id.clone(),
            protocol: self.adapter.protocol_family(),
            winner,
            penalty_applied,
            penalized_candidates,
            duration_ms: Some(race_start.elapsed().as_millis() as u64),
            all_failed,
            candidates,
        }
        .masked()
        .to_header_value()
        .ok()
    }

    async fn record_stats(
        &self,
        states: &HashMap<String, Arc<Mutex<CandidateState>>>,
        race_start: Instant,
        winner: Option<String>,
        buffer_events: u64,
        penalty_applied: bool,
        errors: BTreeMap<String, String>,
    ) {
        let first_content_times = participants_first_content_times(states, race_start).await;
        self.runtime.with_stats(|stats| {
            stats.add(
                self.group.id.clone(),
                self.adapter.protocol_family(),
                winner,
                race_start.elapsed().as_millis() as u64,
                buffer_events,
                states.keys().cloned().collect(),
                first_content_times,
                penalty_applied,
                errors,
            );
        });
    }
}

#[derive(Debug, Clone)]
struct PreparedParticipant {
    participant: RaceParticipant,
    relative_delay: Duration,
}

#[derive(Debug, Clone)]
struct CandidateLoopSnapshot {
    name: String,
    launched_at: Option<Instant>,
    first_content_at: Option<Instant>,
    failed: bool,
    ended: bool,
    usable: bool,
}

fn launch_candidates(
    schedule: Vec<ScheduledCandidate>,
    participants: Vec<RaceParticipant>,
    states: HashMap<String, Arc<Mutex<CandidateState>>>,
    senders: HashMap<String, mpsc::Sender<Bytes>>,
    stream_factory: Arc<dyn CandidateStreamFactory>,
    adapter: Arc<dyn RaceProtocolAdapter>,
    shared_buffered_events: Arc<AtomicUsize>,
    shared_buffered_bytes: Arc<AtomicUsize>,
    winner_signal: Arc<Notify>,
    execution_settings: RaceExecutionSettings,
    tasks: &mut HashMap<String, JoinHandle<()>>,
) {
    let participants_by_name = participants
        .into_iter()
        .map(|participant| (participant.candidate.name.clone(), participant))
        .collect::<HashMap<_, _>>();

    let mut launch_offset = Duration::ZERO;
    for scheduled in schedule {
        launch_offset += scheduled.relative_delay;

        let Some(participant) = participants_by_name.get(&scheduled.candidate.name).cloned() else {
            continue;
        };
        let Some(state) = states.get(&scheduled.candidate.name).cloned() else {
            continue;
        };
        let Some(sender) = senders.get(&scheduled.candidate.name).cloned() else {
            continue;
        };
        let adapter_ref = adapter.clone();
        let stream_factory = stream_factory.clone();
        let participant_name = scheduled.candidate.name.clone();
        let shared_buffered_events = shared_buffered_events.clone();
        let shared_buffered_bytes = shared_buffered_bytes.clone();
        let winner_signal = winner_signal.clone();
        let launch_delay = launch_offset;
        let task = tokio::spawn(async move {
            if !launch_delay.is_zero() {
                tokio::time::sleep(launch_delay).await;
            }
            debug!(
                candidate = %participant.candidate.name,
                upstream_model = %participant.candidate.upstream_model,
                delay_ms = launch_delay.as_millis(),
                "launching race candidate"
            );
            state.lock().await.launched_at = Some(Instant::now());
            winner_signal.notify_one();
            let event_stream = match stream_factory.open_stream(participant.clone()).await {
                Ok(stream) => stream,
                Err(error) => {
                    let mut guard = state.lock().await;
                    guard.failed = true;
                    guard.ended = true;
                    guard.error = Some(error.to_string());
                    warn!(
                        candidate = %guard.candidate.name,
                        error = %guard.error.as_deref().unwrap_or("unknown"),
                        "failed to open upstream candidate stream"
                    );
                    winner_signal.notify_one();
                    return;
                }
            };
            drain_candidate(
                state.clone(),
                sender,
                event_stream,
                adapter_ref,
                shared_buffered_events,
                shared_buffered_bytes,
                winner_signal,
                execution_settings,
            )
            .await;
        });
        tasks.insert(participant_name, task);
    }
}

async fn drain_candidate(
    state: Arc<Mutex<CandidateState>>,
    sender: mpsc::Sender<Bytes>,
    mut stream: Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>,
    adapter: Arc<dyn RaceProtocolAdapter>,
    shared_buffered_events: Arc<AtomicUsize>,
    shared_buffered_bytes: Arc<AtomicUsize>,
    winner_signal: Arc<Notify>,
    execution_settings: RaceExecutionSettings,
) {
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                if !queue_event(
                    state.clone(),
                    sender.clone(),
                    event,
                    adapter.clone(),
                    shared_buffered_events.clone(),
                    shared_buffered_bytes.clone(),
                    winner_signal.clone(),
                    execution_settings,
                )
                .await
                {
                    break;
                }
            }
            Err(error) => {
                let mut guard = state.lock().await;
                guard.failed = true;
                guard.error = Some(error.to_string());
                guard.ended = true;
                warn!(
                    candidate = %guard.candidate.name,
                    error = %guard.error.as_deref().unwrap_or("unknown"),
                    "candidate stream failed while reading"
                );
                winner_signal.notify_one();
                return;
            }
        }
    }

    state.lock().await.ended = true;
    winner_signal.notify_one();
}

async fn queue_event(
    state: Arc<Mutex<CandidateState>>,
    sender: mpsc::Sender<Bytes>,
    event: Bytes,
    adapter: Arc<dyn RaceProtocolAdapter>,
    shared_buffered_events: Arc<AtomicUsize>,
    shared_buffered_bytes: Arc<AtomicUsize>,
    winner_signal: Arc<Notify>,
    execution_settings: RaceExecutionSettings,
) -> bool {
    let winner_selected = state.lock().await.winner_selected;

    let send_result = if winner_selected {
        sender
            .send(event.clone())
            .await
            .map_err(|error| error.to_string())
    } else {
        match tokio::time::timeout(
            Duration::from_millis(execution_settings.buffer_backpressure_timeout_ms),
            sender.send(event.clone()),
        )
        .await
        {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(_) => Err("buffer overflow".to_string()),
        }
    };

    if let Err(error) = send_result {
        let mut guard = state.lock().await;
        if !guard.winner_selected {
            guard.failed = true;
            guard.error = Some(error);
            warn!(
                candidate = %guard.candidate.name,
                error = %guard.error.as_deref().unwrap_or("unknown"),
                "candidate queue send failed before winner selection"
            );
        }
        guard.ended = true;
        winner_signal.notify_one();
        return false;
    }

    let mut guard = state.lock().await;
    let analysis = adapter.inspect_event(&mut guard, event.as_ref());
    let mut should_notify = false;
    if analysis.has_content && guard.first_content_at.is_none() {
        guard.first_content_at = Some(Instant::now());
        should_notify = true;
        info!(
            candidate = %guard.candidate.name,
            "candidate produced first usable content"
        );
    }
    if !guard.winner_selected {
        guard.buffered_count += 1;
        guard.buffered_bytes += event.len();
        let total_buffered = shared_buffered_events.fetch_add(1, Ordering::Relaxed) + 1;
        let total_buffered_bytes =
            shared_buffered_bytes.fetch_add(event.len(), Ordering::Relaxed) + event.len();
        if total_buffered > execution_settings.max_buffer_events
            || total_buffered_bytes > execution_settings.max_buffer_bytes
        {
            guard.failed = true;
            guard.error = Some(if total_buffered > execution_settings.max_buffer_events {
                "buffer overflow".to_string()
            } else {
                "buffer bytes overflow".to_string()
            });
            guard.ended = true;
            warn!(
                candidate = %guard.candidate.name,
                total_buffered,
                total_buffered_bytes,
                max_buffer_events = execution_settings.max_buffer_events,
                max_buffer_bytes = execution_settings.max_buffer_bytes,
                "candidate dropped because shared race buffer overflowed"
            );
            should_notify = true;
            drop(guard);
            if should_notify {
                winner_signal.notify_one();
            }
            return false;
        }
    }

    drop(guard);
    if should_notify {
        winner_signal.notify_one();
    }
    true
}

async fn find_winner(
    group: &RaceGroup,
    participants: &[PreparedParticipant],
    states: &HashMap<String, Arc<Mutex<CandidateState>>>,
    winner_signal: Arc<Notify>,
    race_start: Instant,
) -> Option<String> {
    let max_duration = Duration::from_millis(group.race_max_wait_time_ms.unwrap_or_else(|| {
        participants
            .iter()
            .map(|participant| {
                participant
                    .participant
                    .candidate
                    .response_protection_timeout_ms
            })
            .max()
            .unwrap_or(1_000)
            * 3
    }));

    loop {
        let notified = winner_signal.notified();
        let now = Instant::now();
        let snapshots = collect_ordered_state_snapshots(participants, states).await;
        if now.duration_since(race_start) > max_duration {
            if let Some(winner) = first_usable_candidate(&snapshots) {
                return Some(winner);
            }
            return None;
        }

        if let Some(winner) = winner_from_snapshots(participants, &snapshots, now) {
            return Some(winner);
        }

        let all_launched = snapshots
            .iter()
            .all(|snapshot| snapshot.launched_at.is_some());
        let all_done = snapshots.iter().all(|snapshot| snapshot.ended);
        let any_usable = snapshots.iter().any(|snapshot| snapshot.usable);
        if all_launched && all_done && !any_usable {
            return None;
        }

        let remaining = max_duration.saturating_sub(now.duration_since(race_start));
        if remaining.is_zero() {
            continue;
        }
        let wake_after = next_winner_recheck_delay(participants, &snapshots, now)
            .unwrap_or(remaining)
            .min(remaining);
        tokio::select! {
            _ = notified => {}
            _ = tokio::time::sleep(wake_after) => {}
        }
    }
}

fn winner_from_snapshots(
    participants: &[PreparedParticipant],
    snapshots: &[CandidateLoopSnapshot],
    now: Instant,
) -> Option<String> {
    for (candidate_index, snapshot) in snapshots.iter().enumerate() {
        if snapshot.failed && !snapshot.usable {
            continue;
        }
        if snapshot.first_content_at.is_none() {
            continue;
        }
        if higher_priority_candidate_still_protected(participants, snapshots, candidate_index, now)
        {
            continue;
        }
        return Some(snapshot.name.clone());
    }
    None
}

fn next_winner_recheck_delay(
    participants: &[PreparedParticipant],
    snapshots: &[CandidateLoopSnapshot],
    now: Instant,
) -> Option<Duration> {
    let mut next_delay: Option<Duration> = None;

    for (candidate_index, snapshot) in snapshots.iter().enumerate() {
        if snapshot.failed && !snapshot.usable {
            continue;
        }
        if snapshot.first_content_at.is_none() {
            continue;
        }

        for (higher, higher_snapshot) in participants
            .iter()
            .zip(snapshots.iter())
            .take(candidate_index)
        {
            if higher_snapshot.failed {
                continue;
            }
            let Some(launched_at) = higher_snapshot.launched_at else {
                continue;
            };
            let protection_window =
                Duration::from_millis(higher.participant.candidate.response_protection_timeout_ms);
            let elapsed = now.duration_since(launched_at);
            if elapsed >= protection_window {
                continue;
            }
            let remaining = protection_window - elapsed;
            next_delay = Some(match next_delay {
                Some(current) => current.min(remaining),
                None => remaining,
            });
        }
    }

    next_delay
}

fn higher_priority_candidate_still_protected(
    participants: &[PreparedParticipant],
    snapshots: &[CandidateLoopSnapshot],
    candidate_index: usize,
    now: Instant,
) -> bool {
    for (higher, snapshot) in participants
        .iter()
        .zip(snapshots.iter())
        .take(candidate_index)
    {
        if snapshot.failed {
            continue;
        }
        let Some(launched_at) = snapshot.launched_at else {
            continue;
        };
        if now.duration_since(launched_at)
            < Duration::from_millis(higher.participant.candidate.response_protection_timeout_ms)
        {
            return true;
        }
    }
    false
}

async fn collect_ordered_state_snapshots(
    participants: &[PreparedParticipant],
    states: &HashMap<String, Arc<Mutex<CandidateState>>>,
) -> Vec<CandidateLoopSnapshot> {
    let mut snapshots = Vec::with_capacity(participants.len());
    for participant in participants {
        let guard = states
            .get(&participant.participant.candidate.name)
            .expect("candidate state")
            .lock()
            .await;
        snapshots.push(CandidateLoopSnapshot {
            name: participant.participant.candidate.name.clone(),
            launched_at: guard.launched_at,
            first_content_at: guard.first_content_at,
            failed: guard.failed,
            ended: guard.ended,
            usable: has_usable_content(&guard),
        });
    }
    snapshots
}

fn first_usable_candidate(snapshots: &[CandidateLoopSnapshot]) -> Option<String> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.usable)
        .map(|snapshot| snapshot.name.clone())
}

fn has_usable_content(state: &CandidateState) -> bool {
    state.first_content_at.is_some() && state.error.as_deref() != Some("buffer overflow")
}

async fn collect_errors(
    states: &HashMap<String, Arc<Mutex<CandidateState>>>,
) -> BTreeMap<String, String> {
    let mut errors = BTreeMap::new();
    for (name, state) in states {
        let guard = state.lock().await;
        if guard.failed || guard.error.is_some() {
            errors.insert(
                name.clone(),
                guard
                    .error
                    .clone()
                    .unwrap_or_else(|| "no content".to_string()),
            );
        }
    }
    if errors.is_empty() {
        for name in states.keys() {
            errors.insert(name.clone(), "no content".to_string());
        }
    }
    errors
}

fn spawn_winner_stream(
    mut winner_receiver: mpsc::Receiver<Bytes>,
    winner_task: JoinHandle<()>,
    winner_state: Arc<Mutex<CandidateState>>,
    participants: HashMap<String, Arc<Mutex<CandidateState>>>,
    runtime: Arc<GroupRuntimeHandle>,
    adapter: Arc<dyn RaceProtocolAdapter>,
    group_id: String,
    winner_name: String,
    observability: Observability,
    protocol_label: String,
    active_race_guard: ActiveRaceGuard,
    penalty_applied: bool,
    race_start: Instant,
) -> Pin<Box<dyn Stream<Item = Bytes> + Send>> {
    let (output_sender, mut output_receiver) = mpsc::channel::<Bytes>(64);

    tokio::spawn(async move {
        let mut client_disconnected = false;

        while let Some(event) = winner_receiver.recv().await {
            if output_sender.send(event).await.is_err() {
                client_disconnected = true;
                break;
            }
        }

        if client_disconnected && !winner_task.is_finished() {
            winner_task.abort();
        }
        let _ = winner_task.await;

        let winner_snapshot = winner_state.lock().await.clone();
        if winner_snapshot.failed && !client_disconnected {
            for event in adapter.fallback_close_events(&group_id, &winner_snapshot) {
                if output_sender.send(event).await.is_err() {
                    break;
                }
            }
        }

        let buffer_events = winner_snapshot.buffered_count as u64;
        let errors = collect_errors(&participants).await;
        let first_content_times = participants_first_content_times(&participants, race_start).await;
        runtime.with_stats(|stats| {
            stats.add(
                group_id.clone(),
                adapter.protocol_family(),
                Some(winner_name.clone()),
                race_start.elapsed().as_millis() as u64,
                buffer_events,
                participants.keys().cloned().collect(),
                first_content_times,
                penalty_applied,
                errors,
            );
        });
        observability.finish_race(&protocol_label, "winner", race_start.elapsed());
        drop(active_race_guard);
    });

    Box::pin(stream! {
        while let Some(event) = output_receiver.recv().await {
            yield event;
        }
    }) as Pin<Box<dyn Stream<Item = Bytes> + Send>>
}

async fn cancel_task(task: JoinHandle<()>) {
    task.abort();
    let _ = task.await;
}

async fn cancel_all_tasks(tasks: HashMap<String, JoinHandle<()>>) {
    for (_, task) in tasks {
        cancel_task(task).await;
    }
}

async fn participants_first_content_times(
    states: &HashMap<String, Arc<Mutex<CandidateState>>>,
    race_start: Instant,
) -> BTreeMap<String, u64> {
    let mut output = BTreeMap::new();
    for (name, state) in states {
        let guard = state.lock().await;
        if let Some(first_content_at) = guard.first_content_at {
            output.insert(
                name.clone(),
                first_content_at.duration_since(race_start).as_millis() as u64,
            );
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use async_stream::stream;
    use futures_util::StreamExt;
    use serde_json::json;

    use super::*;
    use crate::adapters::OpenAiAdapter;
    use crate::domain::DownstreamRouteKind;
    use crate::observability::Observability;
    use crate::runtime::RuntimeRegistry;

    fn candidate(name: &str, weight: f64, timeout_ms: u64) -> RaceCandidate {
        RaceCandidate {
            id: name.to_string(),
            group_id: "group-a".to_string(),
            name: name.to_string(),
            model_id: None,
            upstream_model: format!("vendor/{name}"),
            inline_endpoint_overrides: vec![],
            initial_weight: weight,
            response_protection_timeout_ms: timeout_ms,
            enabled: true,
            metadata: json!({}),
        }
    }

    fn participant(name: &str, weight: f64, timeout_ms: u64) -> PreparedParticipant {
        PreparedParticipant {
            participant: RaceParticipant {
                candidate: candidate(name, weight, timeout_ms),
                endpoint: RaceTargetEndpoint {
                    protocol_family: crate::domain::ProtocolFamily::OpenAi,
                    base_url: "https://example.com/v1".to_string(),
                    auth_strategy: crate::domain::AuthStrategy::Bearer,
                    key_pool_id: "pool-a".to_string(),
                    request_timeout_ms: Some(30_000),
                    extra_headers: Default::default(),
                    extra_query: Default::default(),
                    enabled: true,
                },
                selected_key: RaceKey {
                    id: "key-a".to_string(),
                    key_pool_id: "pool-a".to_string(),
                    secret: "secret".to_string(),
                    enabled: true,
                    metadata: json!({}),
                },
                masked_key: "secret***".to_string(),
            },
            relative_delay: Duration::ZERO,
        }
    }

    fn state(candidate: &RaceCandidate) -> Arc<Mutex<CandidateState>> {
        Arc::new(Mutex::new(CandidateState::new(
            candidate.clone(),
            "secret***".to_string(),
            Duration::ZERO,
            CandidateWeightSnapshot {
                initial_weight: candidate.initial_weight,
                effective_weight: candidate.initial_weight,
                weight_deviation: 0.0,
                status: "normal".to_string(),
            },
        )))
    }

    #[tokio::test]
    async fn higher_ranked_candidate_blocks_within_protection_window() {
        let high = participant("high", 100.0, 5_000);
        let low = participant("low", 90.0, 5_000);
        let participants = vec![high.clone(), low.clone()];
        let race_start = Instant::now();

        let high_state = state(&high.participant.candidate);
        let low_state = state(&low.participant.candidate);

        {
            let mut guard = high_state.lock().await;
            guard.launched_at = Some(race_start);
        }
        {
            let mut guard = low_state.lock().await;
            guard.launched_at = Some(race_start + Duration::from_millis(10));
            guard.first_content_at = Some(race_start + Duration::from_millis(20));
        }

        let states = HashMap::from([
            ("high".to_string(), high_state.clone()),
            ("low".to_string(), low_state.clone()),
        ]);

        let early = collect_ordered_state_snapshots(&participants, &states).await;
        assert!(higher_priority_candidate_still_protected(
            &participants,
            &early,
            1,
            race_start + Duration::from_millis(100),
        ));

        let late = collect_ordered_state_snapshots(&participants, &states).await;
        assert!(!higher_priority_candidate_still_protected(
            &participants,
            &late,
            1,
            race_start + Duration::from_millis(5_100),
        ));
    }

    #[tokio::test]
    async fn failed_but_usable_content_can_still_win_on_timeout() {
        let high = participant("high", 100.0, 5_000);
        let low = participant("low", 90.0, 5_000);
        let participants = vec![high.clone(), low.clone()];
        let race_start = Instant::now() - Duration::from_millis(16_000);

        let high_state = state(&high.participant.candidate);
        {
            let mut guard = high_state.lock().await;
            guard.launched_at = Some(race_start);
            guard.first_content_at = Some(race_start + Duration::from_millis(300));
            guard.failed = true;
            guard.error = Some("connection reset".to_string());
        }
        let low_state = state(&low.participant.candidate);

        let states = HashMap::from([
            ("high".to_string(), high_state),
            ("low".to_string(), low_state),
        ]);

        let group = RaceGroup {
            id: "group-a".to_string(),
            display_name: "Group A".to_string(),
            fallback_ratio: 0.5,
            decay_factor: 0.8,
            penalty_rate: 5.0,
            recovery_rate: 0.1,
            race_max_wait_time_ms: Some(15_000),
            enabled: true,
            candidates: vec![
                high.participant.candidate.clone(),
                low.participant.candidate.clone(),
            ],
        };

        let winner = find_winner(
            &group,
            &participants,
            &states,
            Arc::new(Notify::new()),
            race_start,
        )
        .await;
        assert_eq!(winner.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn find_winner_reacts_without_fixed_poll_delay() {
        let fast = participant("fast", 100.0, 1_000);
        let participants = vec![fast.clone()];
        let race_start = Instant::now();
        let fast_state = state(&fast.participant.candidate);
        let states = HashMap::from([("fast".to_string(), fast_state.clone())]);
        let winner_signal = Arc::new(Notify::new());

        let group = RaceGroup {
            id: "group-a".to_string(),
            display_name: "Group A".to_string(),
            fallback_ratio: 0.0,
            decay_factor: 1.0,
            penalty_rate: 5.0,
            recovery_rate: 0.1,
            race_max_wait_time_ms: Some(500),
            enabled: true,
            candidates: vec![fast.participant.candidate.clone()],
        };

        let state_for_task = fast_state.clone();
        let signal_for_task = winner_signal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            let mut guard = state_for_task.lock().await;
            guard.launched_at = Some(Instant::now());
            guard.first_content_at = Some(Instant::now());
            drop(guard);
            signal_for_task.notify_one();
        });

        let started = Instant::now();
        let winner = find_winner(&group, &participants, &states, winner_signal, race_start).await;
        assert_eq!(winner.as_deref(), Some("fast"));
        assert!(started.elapsed() < Duration::from_millis(40));
    }

    #[tokio::test]
    async fn oversized_buffered_event_is_rejected_by_byte_cap() {
        let candidate = candidate("winner", 100.0, 1_000);
        let state = state(&candidate);
        let (sender, _receiver) = mpsc::channel(4);
        let accepted = queue_event(
            state.clone(),
            sender,
            Bytes::from(vec![b'a'; 128]),
            Arc::new(OpenAiAdapter::new(
                DownstreamRouteKind::OpenAiChatCompletions,
            )),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            Arc::new(Notify::new()),
            RaceExecutionSettings {
                max_buffer_events: 10,
                max_buffer_bytes: 32,
                buffer_backpressure_timeout_ms: 100,
            },
        )
        .await;

        assert!(!accepted);
        let snapshot = state.lock().await.clone();
        assert!(snapshot.failed);
        assert_eq!(snapshot.error.as_deref(), Some("buffer bytes overflow"));
    }

    #[derive(Clone)]
    struct MockFactory {
        events: Vec<(Bytes, u64)>,
    }

    #[async_trait]
    impl CandidateStreamFactory for MockFactory {
        async fn open_stream(
            &self,
            _participant: RaceParticipant,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>> {
            let events = self.events.clone();
            Ok(Box::pin(stream! {
                for (event, delay_ms) in events {
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    yield Ok(event);
                }
            }))
        }
    }

    #[tokio::test]
    async fn dropping_client_stream_still_records_stats() {
        let candidate = candidate("winner", 100.0, 1_000);
        let group = RaceGroup {
            id: "group-a".to_string(),
            display_name: "Group A".to_string(),
            fallback_ratio: 0.0,
            decay_factor: 1.0,
            penalty_rate: 5.0,
            recovery_rate: 0.0,
            race_max_wait_time_ms: Some(2_000),
            enabled: true,
            candidates: vec![candidate.clone()],
        };
        let runtime = RuntimeRegistry::new().ensure_group(&group);
        let participant = participant("winner", 100.0, 1_000).participant;
        let observability = Observability::new().expect("observability");
        let factory = Arc::new(MockFactory {
            events: vec![
                (
                    Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n"),
                    0,
                ),
                (
                    Bytes::from_static(b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}]}\n\n"),
                    20,
                ),
            ],
        });

        let execution = RaceCore::new(
            group,
            vec![participant],
            runtime.clone(),
            Arc::new(OpenAiAdapter::new(
                DownstreamRouteKind::OpenAiChatCompletions,
            )),
            "openai".to_string(),
            observability.clone(),
            false,
            RaceExecutionSettings::default(),
        )
        .execute(factory, observability.start_race("openai"))
        .await
        .expect("execute race");

        let mut stream = execution.stream;
        let first_event = stream.next().await.expect("first winner event");
        assert!(std::str::from_utf8(first_event.as_ref()).is_ok_and(|text| text.contains("hello")));
        drop(stream);

        tokio::time::sleep(Duration::from_millis(80)).await;

        let snapshot = runtime.stats_snapshot();
        assert_eq!(snapshot.total_races, 1);
        assert_eq!(snapshot.recent_races.len(), 1);
        assert_eq!(snapshot.recent_races[0].winner.as_deref(), Some("winner"));
    }
}
