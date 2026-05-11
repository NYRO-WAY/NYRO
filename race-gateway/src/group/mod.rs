mod diagnostics;
mod race_core;
mod scheduler;
mod stats;
mod types;
mod weight_tracker;

pub use diagnostics::{
    MAX_DIAGNOSTIC_ERROR_CHARS, RACE_DIAGNOSTICS_HEADER, RaceDiagnosticsSink, mask_key,
    truncate_error,
};
pub use race_core::{
    BUFFER_BACKPRESSURE_TIMEOUT_MS, CandidateState, CandidateStreamFactory, MAX_BUFFER_BYTES,
    MAX_BUFFER_EVENTS, ProtocolFlags, RaceCore, RaceExecutionSettings, RaceParticipant,
    RaceStreamExecution,
};
pub use scheduler::{ScheduledCandidate, compute_schedule};
pub use stats::RaceStats;
pub use types::{
    CandidateDiagnosticsPayload, CandidateWeightSnapshot, ProtocolStatsSnapshot,
    RaceDiagnosticsHeaderPayload, RaceRecord, RaceStatsSnapshot,
};
pub use weight_tracker::WeightTracker;
