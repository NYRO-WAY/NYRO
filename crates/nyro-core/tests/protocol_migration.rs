//! Acceptance: the SQLite startup migration rewrites legacy / alias
//! protocol identifiers in `providers` rows into canonical
//! [`ProtocolId`](nyro_core::protocol::ids::ProtocolId) form.
//!
//! Two guarantees:
//!
//! 1. **First migration normalizes** — a row stored by an older build
//!    (`default_protocol = "openai"`, endpoint keys `"openai"` /
//!    `"openai_responses"` / `"anthropic"` / `"gemini"`) gets rewritten
//!    to the canonical form.
//! 2. **Second migration is a no-op** — re-running `migrate` on an
//!    already-normalized DB does not change any rows. We assert this
//!    by snapshotting the row state before and after.

use nyro_core::db::{init_pool, migrate};
use nyro_core::protocol::ids::{
    ANTHROPIC_MESSAGES_2023_06_01, GOOGLE_GENERATE_V1BETA, OPENAI_CHAT_V1, OPENAI_RESPONSES_V1,
};
use sqlx::Row;

#[tokio::test]
async fn migration_normalizes_legacy_protocol_keys_then_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(dir.path()).await.unwrap();

    // First migration → creates schema.
    migrate(&pool, 8).await.unwrap();

    // Seed a provider with legacy strings, mimicking a row written by
    // an older build before the canonical-id rollout.
    sqlx::query(
        "INSERT INTO providers (id, name, protocol, base_url, api_key, default_protocol, protocol_endpoints) \
         VALUES ('p1', 'p1', 'openai', 'https://a.example/v1', 'k', 'openai', \
         '{\"openai\":{\"base_url\":\"https://a.example/v1\"},\"openai_responses\":{\"base_url\":\"https://b.example/v1\"},\"anthropic\":{\"base_url\":\"https://c.example/v1\"},\"gemini\":{\"base_url\":\"https://d.example/v1\"}}')"
    )
    .execute(&pool)
    .await
    .unwrap();

    // Second migration → must rewrite the legacy row.
    migrate(&pool, 8).await.unwrap();

    let row = sqlx::query("SELECT default_protocol, protocol_endpoints FROM providers WHERE id = 'p1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let default_protocol: String = row.try_get("default_protocol").unwrap();
    let endpoints_raw: String = row.try_get("protocol_endpoints").unwrap();
    let endpoints: serde_json::Value = serde_json::from_str(&endpoints_raw).unwrap();
    let obj = endpoints.as_object().unwrap();

    assert_eq!(default_protocol, OPENAI_CHAT_V1.to_string());
    assert!(obj.contains_key(&OPENAI_CHAT_V1.to_string()));
    assert!(obj.contains_key(&OPENAI_RESPONSES_V1.to_string()));
    assert!(obj.contains_key(&ANTHROPIC_MESSAGES_2023_06_01.to_string()));
    assert!(obj.contains_key(&GOOGLE_GENERATE_V1BETA.to_string()));
    // Legacy keys must be gone.
    assert!(!obj.contains_key("openai"));
    assert!(!obj.contains_key("openai_responses"));
    assert!(!obj.contains_key("anthropic"));
    assert!(!obj.contains_key("gemini"));

    // Snapshot the current state, then run migrate() again and verify
    // nothing changes — the migration must be idempotent.
    let snapshot_before = sqlx::query("SELECT id, default_protocol, protocol_endpoints FROM providers ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| {
            (
                r.get::<String, _>("id"),
                r.get::<String, _>("default_protocol"),
                r.get::<String, _>("protocol_endpoints"),
            )
        })
        .collect::<Vec<_>>();

    migrate(&pool, 8).await.unwrap();

    let snapshot_after = sqlx::query("SELECT id, default_protocol, protocol_endpoints FROM providers ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| {
            (
                r.get::<String, _>("id"),
                r.get::<String, _>("default_protocol"),
                r.get::<String, _>("protocol_endpoints"),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        snapshot_before, snapshot_after,
        "second migrate() must be a no-op on already-normalized rows"
    );
}

#[tokio::test]
async fn migration_preserves_unknown_keys() {
    let dir = tempfile::tempdir().unwrap();
    let pool = init_pool(dir.path()).await.unwrap();
    migrate(&pool, 8).await.unwrap();

    sqlx::query(
        "INSERT INTO providers (id, name, protocol, base_url, api_key, default_protocol, protocol_endpoints) \
         VALUES ('p2', 'p2', 'openai', 'https://a.example/v1', 'k', 'openai', \
         '{\"openai\":{\"base_url\":\"a\"},\"unknown/dialect/v9\":{\"base_url\":\"x\"}}')"
    )
    .execute(&pool)
    .await
    .unwrap();
    migrate(&pool, 8).await.unwrap();

    let raw: String = sqlx::query_scalar("SELECT protocol_endpoints FROM providers WHERE id = 'p2'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(v.get(OPENAI_CHAT_V1.to_string().as_str()).is_some());
    assert!(
        v.get("unknown/dialect/v9").is_some(),
        "unknown keys must round-trip unchanged so foreign data isn't dropped"
    );
}
