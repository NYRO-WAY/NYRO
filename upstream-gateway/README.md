# upstream-gateway

`upstream-gateway` is a new standalone service planned for this repository.

Its purpose is to sit between `nyro` and the real upstream model vendors:

```text
client/sdk/cli -> nyro -> upstream-gateway -> real upstream provider
```

It is intentionally **not** a second protocol-conversion engine.

Responsibilities:

- accept already-native upstream protocol requests from `nyro`
- select an upstream API key from a provider-bound key pool
- enforce upstream RPM / RPD / TPM limits
- estimate provider-native input tokens before dispatch
- stream upstream responses through without aggregating them
- expose runtime snapshot and admin APIs for rate-limit observability

Non-responsibilities:

- no OpenAI / Anthropic / Gemini cross-protocol conversion
- no duplication of `nyro`'s InternalRequest / codec / handler stack
- no direct coupling to `nyro-core` runtime internals

Current status:

- this directory is design-first
- it currently builds as an independent crate and is not added to the root Rust workspace
- limiter core extraction is complete and covered by regression tests
- provider/config store contracts and request metadata extraction are in place
- SQLite-backed config persistence is now in place for providers, keys, model rules, and settings shell tables
- startup can seed provider bundles from a bootstrap JSON file when the database is empty
- non-stream upstream passthrough is working end-to-end for configured providers
- stream passthrough is working with conservative post-stream settlement
- input-token estimation is now wired into admission for TPM checks
- OpenAI / Anthropic currently use conservative body-based tokenizer counting
- Gemini now uses structured `gemini-tokenizer` counting, with explicit tokenizer-model fallback and conservative `inlineData` compensation
- non-stream post-response settlement now uses provider-native `usage` fields when available
- control-plane CRUD is available for provider bundles, keys, and model rules
- provider-level runtime snapshot APIs are now available for WebUI consumption
- standalone admin panel is now served directly by `upstream-gateway` at `/admin`
- stream settlement still falls back conservatively when upstream usage is absent from the stream
- Gemini-first Nyro cutover example now lives in [examples/gemini-first](./examples/gemini-first)
- Gemini-first rollout guide now lives in [docs/gemini-first-cutover.md](./docs/gemini-first-cutover.md)
- formal architecture lives in [docs/architecture.md](./docs/architecture.md)
- ordered implementation checklist lives in [docs/development-tasks.md](./docs/development-tasks.md)
- old implementation extraction inventory lives in [docs/reuse-inventory.md](./docs/reuse-inventory.md)

Build note:

- on Windows MSVC, build from the `upstream-gateway/` directory so its local [`.cargo/config.toml`](./.cargo/config.toml) is applied
- if `cmake` is not on `PATH`, set `CMAKE=/absolute/path/to/cmake.exe` before running `cargo build`, `cargo test`, or `cargo run`

Startup config:

- `UPSTREAM_GATEWAY_PROXY_BIND_ADDR`
- `UPSTREAM_GATEWAY_ADMIN_BIND_ADDR` (optional; unset means admin listener disabled)
- `UPSTREAM_GATEWAY_BIND_ADDR` (legacy proxy-only compatibility input)
- `UPSTREAM_GATEWAY_REQUEST_TIMEOUT_SECS`
- `UPSTREAM_GATEWAY_DATABASE_URL`
- `UPSTREAM_GATEWAY_BOOTSTRAP_JSON_PATH`

Recommended local defaults:

- proxy/data plane: `127.0.0.1:2080`
- admin/control plane when enabled: `127.0.0.1:2081`

Current bootstrap file format:

```json
{
  "providers": [
    {
      "provider": {
        "id": "gemini-prod",
        "name": "Gemini Prod",
        "vendor": "gemini",
        "base_url": "https://generativelanguage.googleapis.com",
        "auth_strategy": {
          "kind": "query_api_key",
          "parameter_name": "key"
        },
        "enabled": true
      },
      "keys": [
        {
          "id": "key-a",
          "provider_id": "gemini-prod",
          "display_name": "Key A",
          "api_key": "gem-test-key-a",
          "enabled": true,
          "weight": 1
        }
      ],
      "model_rules": [
        {
          "provider_id": "gemini-prod",
          "model": "*",
          "rpm": 60,
          "rpd": 5000,
          "tpm": 120000,
          "tpm_mode": "input_only",
          "tokenizer_model": "gemini-2.5-pro"
        }
      ],
      "daily_reset": {
        "timezone": "+08:00",
        "hour": 4,
        "minute": 0
      }
    }
  ]
}
```

Bootstrap JSON remains the fastest way to seed a fresh SQLite config store, but provider, key, and model-rule CRUD is also available through the control plane and `/admin`.

Current control-plane endpoints:

- `GET /admin`
- `GET /admin/providers`
- `GET /admin/providers/:provider_id`
- `PUT /admin/providers/:provider_id`
- `DELETE /admin/providers/:provider_id`
- `PUT /admin/providers/:provider_id/keys/:key_id`
- `DELETE /admin/providers/:provider_id/keys/:key_id`
- `PUT /admin/providers/:provider_id/model-rules/*model`
- `DELETE /admin/providers/:provider_id/model-rules/*model`
- `GET /admin/providers/:provider_id/runtime`
- `GET /admin/runtime/providers`

Note:

- `/admin` and the control-plane endpoints above only exist when `UPSTREAM_GATEWAY_ADMIN_BIND_ADDR` is explicitly configured

Current standalone admin panel sections:

- Provider management
- Runtime observability
- Chinese React WebUI under `webui/`

The panel is built from `webui/dist` and is served from:

- `GET /admin`
- `GET /admin/assets/*`

Recommended listener split:

- proxy listener
  - `/healthz`
  - `/providers/:provider_id/...`
- admin listener
  - `/admin`
  - `/admin/healthz`
  - `/admin/providers/...`
