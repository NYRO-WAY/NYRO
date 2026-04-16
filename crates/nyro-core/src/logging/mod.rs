use tokio::sync::mpsc;

use crate::protocol::types::TokenUsage;
use crate::storage::DynStorage;

const DEFAULT_RETENTION_DAYS: i64 = 30;

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
}

pub async fn run_collector(mut rx: mpsc::Receiver<LogEntry>, storage: DynStorage) {
    let mut buffer: Vec<LogEntry> = Vec::with_capacity(64);
    let mut flush_interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(3600));

    loop {
        tokio::select! {
            Some(entry) = rx.recv() => {
                buffer.push(entry);
                if buffer.len() >= 64 {
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
        .get("log_retention_days")
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

async fn flush(storage: DynStorage, buffer: &mut Vec<LogEntry>) {
    let entries = std::mem::take(buffer);
    let _ = storage.logs().append_batch(entries).await;
}
