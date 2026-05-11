# Gemini-first cutover

This document defines the recommended first migration slice:

```text
client -> nyro -> upstream-gateway -> Gemini
```

## 1. Why Gemini first

Gemini is the best first family to switch because:

- `upstream-gateway` already has provider-native Gemini request passthrough
- Gemini input-token estimation is already wired to `gemini-tokenizer`
- a single Nyro provider endpoint can be repointed without changing Nyro protocol conversion logic

## 2. Responsibility split

Nyro keeps:

- Anthropic / OpenAI / Gemini protocol conversion
- route selection
- client-facing proxy surface

`upstream-gateway` takes over:

- Gemini key pool management
- RPM / RPD / TPM enforcement
- input-token estimation on final Gemini-native JSON
- runtime occupancy and limiter WebUI

## 3. Endpoint shape

Nyro should point its Gemini egress endpoint at:

```text
http://127.0.0.1:2080/providers/gemini-prod/google
```

Nyro will still append its normal Gemini egress path:

- non-stream: `/v1beta/models/{model}:generateContent`
- stream: `/v1beta/models/{model}:streamGenerateContent?alt=sse`

So the final upstream-gateway ingress becomes:

```text
/providers/gemini-prod/google/v1beta/models/{model}:generateContent
/providers/gemini-prod/google/v1beta/models/{model}:streamGenerateContent
```

## 4. Config mapping

### 4.1 upstream-gateway

Use a persisted provider bundle for the real Gemini upstream:

- provider id: `gemini-prod`
- vendor: `gemini`
- base URL: `https://generativelanguage.googleapis.com`
- auth strategy: query parameter `key`
- keys: real Gemini API keys only live here
- model rules: attach RPM / RPD / TPM here

Reference file:

- [bootstrap.template.json](../examples/gemini-first/bootstrap.template.json)

### 4.2 Nyro

Use a normal Nyro provider that targets the gateway instead of Google directly:

- protocol: `gemini`
- endpoint base URL: `http://127.0.0.1:2080/providers/gemini-prod/google`
- route target model: keep the real Gemini model id

Reference file:

- [nyro.standalone.yaml](../examples/gemini-first/nyro.standalone.yaml)

Important:

- Nyro provider `api_key` is only the Nyro -> upstream-gateway hop credential placeholder
- real vendor API keys must not stay in Nyro for this provider
- real vendor API keys belong in the upstream-gateway key pool

## 5. Validation flow

The included validation script exercises:

1. bootstrap persisted gateway config into SQLite
2. start `upstream-gateway`
3. start Nyro standalone mode with Gemini egress pointing to `upstream-gateway`
4. send Anthropic-format traffic to Nyro `/v1/messages`
5. verify Nyro converts to Gemini and the gateway forwards to real Gemini
6. verify stream mode still returns SSE
7. read gateway runtime snapshot

Script:

- [validate.ps1](../examples/gemini-first/validate.ps1)

Required environment:

- `GEMINI_API_KEY`

Run:

```powershell
cd D:\Dev\project\xyz\nyro-codex\nyro\upstream-gateway\examples\gemini-first
$env:GEMINI_API_KEY = "your-real-gemini-key"
.\validate.ps1
```

## 6. Rollout recommendation

Use this order:

1. keep OpenAI and Anthropic providers on their current direct upstreams
2. create one Gemini provider in `upstream-gateway`
3. repoint only the Nyro Gemini provider endpoint to `upstream-gateway`
4. validate one production route first
5. watch `/admin/runtime/providers`
6. after Gemini is stable, migrate the next family

## 7. Rollback

Rollback is only a Nyro provider endpoint change:

1. point the Nyro Gemini provider back to the old direct Google base URL
2. keep `upstream-gateway` data intact
3. investigate limiter/runtime behavior offline

No Nyro protocol-conversion code rollback is required.
