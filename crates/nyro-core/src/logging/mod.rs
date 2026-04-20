use tokio::sync::mpsc;

use crate::protocol::types::TokenUsage;
use crate::storage::DynStorage;

const DEFAULT_RETENTION_DAYS: i64 = 7;
const DEFAULT_RECORD_PAYLOADS: bool = true;
pub const LOG_RECORD_PAYLOADS_KEY: &str = "log_record_payloads";
pub const LOG_RETENTION_DAYS_KEY: &str = "log_retention_days";

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub api_key_id: Option<String>,
    pub ingress_protocol: String,
    pub egress_protocol: String,
    pub request_model: String,
    pub actual_model: String,
    pub provider_name: String,
    pub status_code: i32,
    pub duration_ms: f64,
    pub usage: TokenUsage,
    pub is_stream: bool,
    pub is_tool_call: bool,
    pub error_message: Option<String>,
    pub response_preview: Option<String>,
    pub method: Option<String>,
    pub path: Option<String>,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_headers: Option<String>,
    pub response_body: Option<String>,
}

pub async fn run_collector(mut rx: mpsc::Receiver<LogEntry>, storage: DynStorage) {
    let mut buffer: Vec<LogEntry> = Vec::with_capacity(32);
    let mut flush_interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(600));

    loop {
        tokio::select! {
            Some(entry) = rx.recv() => {
                buffer.push(entry);
                if buffer.len() >= 32 {
                    flush(storage.clone(), &mut buffer).await;
                }
            }
            _ = flush_interval.tick() => {
                if !buffer.is_empty() {
                    flush(storage.clone(), &mut buffer).await;
                }
            }
            _ = cleanup_interval.tick() => {
                cleanup_old_logs(storage.clone()).await;
            }
        }
    }
}

async fn cleanup_old_logs(storage: DynStorage) {
    let days = storage
        .settings()
        .get(LOG_RETENTION_DAYS_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);

    let cutoff = format!("-{days} days");
    if let Ok(deleted) = storage.logs().cleanup_before(&cutoff).await {
        if deleted > 0 {
            tracing::info!("cleaned up {deleted} logs older than {days} days");
        }
    }
}

async fn read_record_payloads(storage: &DynStorage) -> bool {
    storage
        .settings()
        .get(LOG_RECORD_PAYLOADS_KEY)
        .await
        .ok()
        .flatten()
        .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "false" | "0" | "off" | "no"))
        .unwrap_or(DEFAULT_RECORD_PAYLOADS)
}

async fn flush(storage: DynStorage, buffer: &mut Vec<LogEntry>) {
    let mut entries = std::mem::take(buffer);
    let record_payloads = read_record_payloads(&storage).await;
    if !record_payloads {
        for entry in entries.iter_mut() {
            entry.request_headers = None;
            entry.request_body = None;
            entry.response_headers = None;
            entry.response_body = None;
        }
    }
    let _ = storage.logs().append_batch(entries).await;
}
