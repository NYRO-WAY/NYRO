# E2E Coverage Matrix

Tracks which `<vendor> × <protocol> × <scenario>` combinations have
recorded fixtures under `tests/e2e/fixtures/`. Each cell maps to a
single `.jsonl` file; presence of the file is what the L2 protocol
matrix tests in `tests/e2e/proxy/` consume.

> **How to update:** when you contribute a fixture, flip the
> corresponding cell to ✓ and link the PR. CI does **not** auto-edit
> this file — it's a human-readable index.

Legend:
- ✓ — recorded
- · — not yet recorded (planned)
- — — not applicable (vendor genuinely lacks the capability)

---

## openai-chat (`/v1/chat/completions`)

| Vendor             | basic-nonstream | basic-stream | tool-use-stream | reasoning-stream |
|--------------------|-----------------|--------------|-----------------|------------------|
| `openai`           | ·               | ·            | ·               | ·                |
| `azure`            | ·               | ·            | ·               | ·                |
| `deepseek`         | ·               | ·            | ·               | ·                |

## openai-responses (`/v1/responses`)

| Vendor             | basic-nonstream | basic-stream | tool-use-stream | reasoning-stream |
|--------------------|-----------------|--------------|-----------------|------------------|
| `openai`           | ·               | ·            | ·               | ·                |
| `azure`            | ·               | ·            | ·               | ·                |

## anthropic-messages (`/v1/messages`)

| Vendor             | basic-nonstream | basic-stream | tool-use-stream | reasoning-stream |
|--------------------|-----------------|--------------|-----------------|------------------|
| `anthropic`        | ·               | ·            | ·               | ·                |
| `deepseek`         | ·               | ·            | ·               | ·                |

## google-content (`/v1beta/models/{model}:{generate,streamGenerate}Content`)

| Vendor             | basic-nonstream | basic-stream | tool-use-stream | reasoning-stream |
|--------------------|-----------------|--------------|-----------------|------------------|
| `google-aistudio`  | ·               | ·            | ·               | ·                |
| `google-vertex`    | ·               | ·            | ·               | ·                |

---

## Roadmap

The matrix above lists the **target** vendor set for the next 12
months. Contribution priority order:

1. `deepseek--openai-chat--*` — minimum CI green path (one cheap
   reasoning + non-reasoning model with full scenario coverage).
2. The remaining `openai-chat` vendors (`openai`, `azure`).
3. `anthropic-messages--anthropic--*`.
4. `google-content--google-aistudio--*`.
5. `openai-responses--openai--*`.
6. Vendor-specific second-protocol coverage
   (`anthropic-messages--deepseek--*`, `google-content--google-vertex--*`).

Adding a new vendor row is fine — open a PR with the fixtures and
the matrix update in the same commit.
