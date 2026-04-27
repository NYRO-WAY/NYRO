use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod fixture;
mod protocol;
mod proxy;
mod record;
mod replay;
mod scenarios;

#[derive(Parser)]
#[command(
    name = "nyro-tools",
    version,
    about = "CLI suite for Nyro e2e testing: proxy / record / replay",
    long_about = "Three subcommands for protocol-conversion testing:\n\
                  - proxy:  transparent passthrough for local debugging\n\
                  - record: scenario-driven recording against real LLM endpoints\n\
                  - replay: persistent stub upstream that replays fixtures by replay_model"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Transparent passthrough proxy (debug only, not for CI)
    Proxy(proxy::ProxyArgs),
    /// Scenario-driven recorder: replays fixed scenarios against a real LLM and writes .jsonl fixtures
    Record(record::RecordArgs),
    /// Persistent stub upstream: serves recorded fixtures via in-memory replay_model HashMap
    Replay(replay::ReplayArgs),
    /// Print scenario metadata (anchor + expected_fields per protocol) as JSON — consumed by pytest
    PrintScenarios,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command {
        Command::Proxy(args) => proxy::run(args).await,
        Command::Record(args) => record::run(args).await,
        Command::Replay(args) => replay::run(args).await,
        Command::PrintScenarios => print_scenarios(),
    }
}

fn print_scenarios() -> Result<()> {
    use protocol::ProtocolKind;
    let protocols = [
        ProtocolKind::OpenAiChat,
        ProtocolKind::OpenAiResponses,
        ProtocolKind::AnthropicMessages,
        ProtocolKind::GoogleContent,
    ];
    let entries: Vec<serde_json::Value> = scenarios::SCENARIOS
        .iter()
        .map(|s| {
            let expected: serde_json::Map<String, serde_json::Value> = protocols
                .iter()
                .map(|p| {
                    (
                        p.as_short_name().to_string(),
                        serde_json::json!(s.expected_fields_for(*p)),
                    )
                })
                .collect();
            serde_json::json!({
                "name": s.name,
                "anchor": s.anchor,
                "stream": s.stream,
                "uses_reasoning_model": s.uses_reasoning_model,
                "expected_fields": expected,
            })
        })
        .collect();
    let body = serde_json::json!({
        "version": fixture::FIXTURE_VERSION,
        "scenarios": entries,
        "protocols": protocols.iter().map(|p| p.as_short_name()).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
