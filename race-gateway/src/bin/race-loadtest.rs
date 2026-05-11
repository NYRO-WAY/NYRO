use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::sync::Semaphore;

#[derive(Debug)]
struct LoadTestConfig {
    url: String,
    method: String,
    requests: usize,
    concurrency: usize,
    timeout_ms: u64,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct Sample {
    status: Option<u16>,
    header_latency_ms: u128,
    first_chunk_latency_ms: Option<u128>,
    total_latency_ms: u128,
    bytes: usize,
    error: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = parse_args()?;
    let client = reqwest::Client::builder()
        .tcp_nodelay(true)
        .timeout(Duration::from_millis(config.timeout_ms))
        .build()
        .context("failed to build reqwest client")?;

    let started_at = Instant::now();
    let semaphore = Arc::new(Semaphore::new(config.concurrency.max(1)));
    let mut handles = Vec::with_capacity(config.requests);

    for _ in 0..config.requests {
        let permit = semaphore.clone().acquire_owned().await?;
        let client = client.clone();
        let url = config.url.clone();
        let method = config.method.clone();
        let headers = config.headers.clone();
        let body = config.body.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            execute_once(client, url, method, headers, body).await
        }));
    }

    let mut samples = Vec::with_capacity(config.requests);
    for handle in handles {
        match handle.await {
            Ok(sample) => samples.push(sample),
            Err(error) => samples.push(Sample {
                status: None,
                header_latency_ms: 0,
                first_chunk_latency_ms: None,
                total_latency_ms: 0,
                bytes: 0,
                error: Some(format!("task join error: {error}")),
            }),
        }
    }

    print_summary(&config, &samples, started_at.elapsed());
    Ok(())
}

async fn execute_once(
    client: reqwest::Client,
    url: String,
    method: String,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
) -> Sample {
    let start = Instant::now();
    let request = client
        .request(method.parse().unwrap_or(reqwest::Method::POST), url)
        .headers(headers)
        .body(body.unwrap_or_default());

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return Sample {
                status: None,
                header_latency_ms: start.elapsed().as_millis(),
                first_chunk_latency_ms: None,
                total_latency_ms: start.elapsed().as_millis(),
                bytes: 0,
                error: Some(error.to_string()),
            };
        }
    };

    let status = response.status().as_u16();
    let header_latency_ms = start.elapsed().as_millis();
    let mut first_chunk_latency_ms = None;
    let mut bytes = 0usize;
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => {
                if first_chunk_latency_ms.is_none() {
                    first_chunk_latency_ms = Some(start.elapsed().as_millis());
                }
                bytes += chunk.len();
            }
            Err(error) => {
                return Sample {
                    status: Some(status),
                    header_latency_ms,
                    first_chunk_latency_ms,
                    total_latency_ms: start.elapsed().as_millis(),
                    bytes,
                    error: Some(error.to_string()),
                };
            }
        }
    }

    Sample {
        status: Some(status),
        header_latency_ms,
        first_chunk_latency_ms,
        total_latency_ms: start.elapsed().as_millis(),
        bytes,
        error: None,
    }
}

fn parse_args() -> anyhow::Result<LoadTestConfig> {
    let mut url = None;
    let mut method = "POST".to_string();
    let mut requests = 100usize;
    let mut concurrency = 10usize;
    let mut timeout_ms = 60_000u64;
    let mut headers = HeaderMap::new();
    let mut body_file = None::<PathBuf>;

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--url" => {
                index += 1;
                url = args.get(index).cloned();
            }
            "--method" => {
                index += 1;
                method = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| "POST".to_string());
            }
            "--requests" => {
                index += 1;
                requests = parse_number(args.get(index), "--requests")?;
            }
            "--concurrency" => {
                index += 1;
                concurrency = parse_number(args.get(index), "--concurrency")?;
            }
            "--timeout-ms" => {
                index += 1;
                timeout_ms = parse_number(args.get(index), "--timeout-ms")?;
            }
            "--body-file" => {
                index += 1;
                body_file = Some(PathBuf::from(
                    args.get(index)
                        .with_context(|| "missing value for --body-file")?,
                ));
            }
            "--header" => {
                index += 1;
                let raw = args
                    .get(index)
                    .with_context(|| "missing value for --header")?;
                let (name, value) = raw
                    .split_once(':')
                    .with_context(|| format!("invalid header '{raw}', expected 'name:value'"))?;
                headers.insert(
                    HeaderName::from_bytes(name.trim().as_bytes())
                        .with_context(|| format!("invalid header name '{name}'"))?,
                    HeaderValue::from_str(value.trim())
                        .with_context(|| format!("invalid header value for '{name}'"))?,
                );
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => bail!("unknown argument '{other}'"),
        }
        index += 1;
    }

    let url = url.with_context(|| "missing required --url")?;
    let body = if let Some(path) = body_file {
        Some(
            std::fs::read(&path)
                .with_context(|| format!("failed to read body file {}", path.display()))?,
        )
    } else {
        None
    };

    Ok(LoadTestConfig {
        url,
        method,
        requests: requests.max(1),
        concurrency: concurrency.max(1),
        timeout_ms: timeout_ms.max(1),
        headers,
        body,
    })
}

fn parse_number<T>(value: Option<&String>, flag: &str) -> anyhow::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw = value.with_context(|| format!("missing value for {flag}"))?;
    raw.parse::<T>()
        .map_err(|error| anyhow::anyhow!("failed to parse {flag}: {error}"))
}

fn print_summary(config: &LoadTestConfig, samples: &[Sample], elapsed: Duration) {
    let success = samples
        .iter()
        .filter(|sample| sample.error.is_none())
        .count();
    let errors = samples.len().saturating_sub(success);
    let throughput = if elapsed.as_secs_f64() > 0.0 {
        samples.len() as f64 / elapsed.as_secs_f64()
    } else {
        samples.len() as f64
    };

    let mut status_counts = BTreeMap::<u16, usize>::new();
    for sample in samples {
        if let Some(status) = sample.status {
            *status_counts.entry(status).or_default() += 1;
        }
    }

    let header = latency_stats(
        samples
            .iter()
            .map(|sample| sample.header_latency_ms)
            .collect(),
    );
    let first_chunk = latency_stats(
        samples
            .iter()
            .filter_map(|sample| sample.first_chunk_latency_ms)
            .collect(),
    );
    let total = latency_stats(
        samples
            .iter()
            .map(|sample| sample.total_latency_ms)
            .collect(),
    );
    let total_bytes = samples.iter().map(|sample| sample.bytes).sum::<usize>();

    println!("race-loadtest summary");
    println!("url: {}", config.url);
    println!(
        "requests: {}  concurrency: {}  elapsed: {:.2}s  throughput: {:.2} req/s",
        config.requests,
        config.concurrency,
        elapsed.as_secs_f64(),
        throughput
    );
    println!(
        "success: {}  errors: {}  bytes: {}",
        success, errors, total_bytes
    );
    if !status_counts.is_empty() {
        let status_summary = status_counts
            .into_iter()
            .map(|(status, count)| format!("{status}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("status: {status_summary}");
    }
    print_latency_line("headers", &header);
    print_latency_line("first_chunk", &first_chunk);
    print_latency_line("total", &total);

    let first_error = samples.iter().find_map(|sample| sample.error.as_deref());
    if let Some(error) = first_error {
        println!("first_error: {error}");
    }
}

fn print_latency_line(label: &str, stats: &LatencyStats) {
    println!(
        "{}_ms: avg={:.2} p50={} p95={} p99={} max={}",
        label, stats.avg, stats.p50, stats.p95, stats.p99, stats.max
    );
}

#[derive(Debug, Default)]
struct LatencyStats {
    avg: f64,
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

fn latency_stats(mut values: Vec<u128>) -> LatencyStats {
    if values.is_empty() {
        return LatencyStats::default();
    }
    values.sort_unstable();
    let sum = values.iter().sum::<u128>();
    LatencyStats {
        avg: sum as f64 / values.len() as f64,
        p50: percentile(&values, 0.50),
        p95: percentile(&values, 0.95),
        p99: percentile(&values, 0.99),
        max: *values.last().unwrap_or(&0),
    }
}

fn percentile(values: &[u128], quantile: f64) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let position = ((values.len() - 1) as f64 * quantile).round() as usize;
    values[position.min(values.len() - 1)]
}

fn print_usage() {
    println!("Usage:");
    println!("  cargo run --bin race-loadtest -- --url <URL> [options]");
    println!();
    println!("Options:");
    println!("  --method <METHOD>           HTTP method, default POST");
    println!("  --requests <N>              Total requests, default 100");
    println!("  --concurrency <N>           Concurrent requests, default 10");
    println!("  --timeout-ms <MS>           Per-request timeout, default 60000");
    println!("  --body-file <PATH>          Raw request body file");
    println!("  --header <name:value>       Repeatable request header");
}
