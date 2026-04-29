/**
 * Protocol identifier utilities.
 *
 * Mirrors the backend `ProtocolId` (`family/dialect/version`) shape and
 * its alias resolution table. The frontend stays loose-typed (`string`
 * on the wire) but routes every comparison and display through these
 * helpers so legacy short names (`openai`, `anthropic`, `gemini`,
 * `openai_responses`) and canonical IDs (`openai/chat/v1`,
 * `anthropic/messages/2023-06-01`, ...) interoperate seamlessly.
 *
 * Keep the alias table in sync with the Rust side at
 * `crates/nyro-core/src/protocol/registry.rs::default_aliases`.
 */

export interface ProtocolId {
  family: string;
  dialect: string;
  version: string;
}

const ALIAS_TABLE: Record<string, ProtocolId> = {
  // Canonical pass-through (idempotent on already-canonical strings).
  "openai/chat/v1": { family: "openai", dialect: "chat", version: "v1" },
  "openai/responses/v1": { family: "openai", dialect: "responses", version: "v1" },
  "openai/embeddings/v1": { family: "openai", dialect: "embeddings", version: "v1" },
  "anthropic/messages/2023-06-01": {
    family: "anthropic",
    dialect: "messages",
    version: "2023-06-01",
  },
  "google/generate/v1beta": { family: "google", dialect: "generate", version: "v1beta" },

  // Short-name aliases used in WebUI dropdowns and YAML configs.
  "openai-chat": { family: "openai", dialect: "chat", version: "v1" },
  "openai-responses": { family: "openai", dialect: "responses", version: "v1" },
  "openai-embeddings": { family: "openai", dialect: "embeddings", version: "v1" },
  "anthropic-messages": {
    family: "anthropic",
    dialect: "messages",
    version: "2023-06-01",
  },
  "google-generate": { family: "google", dialect: "generate", version: "v1beta" },

  // Legacy short names persisted in older databases / preset JSON.
  openai: { family: "openai", dialect: "chat", version: "v1" },
  openai_responses: { family: "openai", dialect: "responses", version: "v1" },
  embeddings: { family: "openai", dialect: "embeddings", version: "v1" },
  anthropic: { family: "anthropic", dialect: "messages", version: "2023-06-01" },
  claude: { family: "anthropic", dialect: "messages", version: "2023-06-01" },
  gemini: { family: "google", dialect: "generate", version: "v1beta" },
};

/** Resolve a raw protocol string into a structured `ProtocolId`, or
 *  `null` if it doesn't match any known alias / canonical form. */
export function parseProtocolId(raw: string | null | undefined): ProtocolId | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const direct = ALIAS_TABLE[trimmed];
  if (direct) return direct;
  // Canonical fallback for unknown but well-formed `family/dialect/version`.
  const parts = trimmed.split("/");
  if (parts.length === 3 && parts.every((p) => p.length > 0)) {
    return { family: parts[0], dialect: parts[1], version: parts[2] };
  }
  return null;
}

/** Format a `ProtocolId` back to its canonical wire form. */
export function formatProtocolId(id: ProtocolId): string {
  return `${id.family}/${id.dialect}/${id.version}`;
}

/** Display label split by the two layers we surface in the UI:
 *  the family (vendor) and a dialect+version subtitle. Returns `null`
 *  when the input can't be parsed so callers can fall back to the
 *  raw string verbatim. */
export function prettyName(raw: string | null | undefined): {
  family: string;
  detail: string;
} | null {
  const id = parseProtocolId(raw);
  if (!id) return null;
  return { family: id.family, detail: `${id.dialect} · ${id.version}` };
}
