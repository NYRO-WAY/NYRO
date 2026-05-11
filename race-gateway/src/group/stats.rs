use std::collections::{BTreeMap, VecDeque};

use chrono::Utc;

use crate::domain::ProtocolFamily;

use super::{ProtocolStatsSnapshot, RaceRecord, RaceStatsSnapshot};

#[derive(Debug, Clone)]
pub struct RaceStats {
    records: VecDeque<RaceRecord>,
    next_id: u64,
    max_records: usize,
}

impl Default for RaceStats {
    fn default() -> Self {
        Self::new(200)
    }
}

impl RaceStats {
    pub fn new(max_records: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(max_records),
            next_id: 1,
            max_records,
        }
    }

    pub fn add(
        &mut self,
        group_id: impl Into<String>,
        protocol: ProtocolFamily,
        winner: Option<String>,
        duration_ms: u64,
        buffer_events: u64,
        participants: Vec<String>,
        first_content_times_ms: BTreeMap<String, u64>,
        penalty_applied: bool,
        errors: BTreeMap<String, String>,
    ) {
        let record = RaceRecord {
            id: self.next_id,
            timestamp: Utc::now(),
            group_id: group_id.into(),
            protocol,
            winner,
            duration_ms,
            buffer_events,
            participants,
            first_content_times_ms,
            penalty_applied,
            errors,
        };
        self.next_id += 1;
        self.records.push_front(record);
        while self.records.len() > self.max_records {
            self.records.pop_back();
        }
    }

    pub fn snapshot(&self) -> RaceStatsSnapshot {
        let records = self.records.iter().cloned().collect::<Vec<_>>();
        let total_races = records.len() as u64;

        let mut winner_distribution = BTreeMap::new();
        let mut protocol_distribution = BTreeMap::new();
        let mut durations = 0u64;
        let mut buffer_events = 0u64;

        for record in &records {
            if let Some(winner) = &record.winner {
                *winner_distribution.entry(winner.clone()).or_insert(0) += 1;
            }
            *protocol_distribution
                .entry(protocol_label(record.protocol).to_string())
                .or_insert(0) += 1;
            durations += record.duration_ms;
            buffer_events += record.buffer_events;
        }

        RaceStatsSnapshot {
            total_races,
            winner_distribution,
            protocol_distribution,
            by_protocol: build_by_protocol(&records),
            avg_race_duration_ms: average_u64(durations, total_races),
            avg_buffer_events: average_u64(buffer_events, total_races),
            recent_races: records,
        }
    }
}

fn build_by_protocol(records: &[RaceRecord]) -> BTreeMap<String, ProtocolStatsSnapshot> {
    let mut grouped = BTreeMap::<String, Vec<&RaceRecord>>::new();
    for record in records {
        grouped
            .entry(protocol_label(record.protocol).to_string())
            .or_default()
            .push(record);
    }

    grouped
        .into_iter()
        .map(|(protocol, items)| {
            let mut wins = BTreeMap::new();
            let mut durations = 0u64;
            let mut buffer_events = 0u64;
            let mut all_failed_count = 0u64;
            for record in &items {
                if let Some(winner) = &record.winner {
                    *wins.entry(winner.clone()).or_insert(0) += 1;
                } else {
                    all_failed_count += 1;
                }
                durations += record.duration_ms;
                buffer_events += record.buffer_events;
            }

            let total = items.len() as u64;
            (
                protocol,
                ProtocolStatsSnapshot {
                    total,
                    wins,
                    avg_race_duration_ms: average_u64(durations, total),
                    avg_buffer_events: average_u64(buffer_events, total),
                    all_failed_count,
                },
            )
        })
        .collect()
}

fn average_u64(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn protocol_label(protocol: ProtocolFamily) -> &'static str {
    match protocol {
        ProtocolFamily::OpenAi => "openai",
        ProtocolFamily::Anthropic => "anthropic",
        ProtocolFamily::Google => "google",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_groups_by_protocol() {
        let mut stats = RaceStats::new(10);
        stats.add(
            "g",
            ProtocolFamily::Anthropic,
            Some("A".to_string()),
            100,
            4,
            vec!["A".to_string(), "B".to_string()],
            BTreeMap::new(),
            true,
            BTreeMap::new(),
        );
        stats.add(
            "g",
            ProtocolFamily::OpenAi,
            None,
            200,
            8,
            vec!["A".to_string(), "B".to_string()],
            BTreeMap::new(),
            false,
            BTreeMap::from([("A".to_string(), "timeout".to_string())]),
        );

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.total_races, 2);
        assert_eq!(snapshot.protocol_distribution.get("anthropic"), Some(&1));
        assert_eq!(
            snapshot
                .by_protocol
                .get("openai")
                .expect("openai stats")
                .all_failed_count,
            1
        );
    }
}
