/**
 * Protocol utilities — mirrors the backend three-layer identity model.
 *
 * Three orthogonal concepts:
 *   Protocol  — suite / wire-format family  (e.g. "openai-compat")
 *   Endpoint  — specific API path           (e.g. "chat-completions")
 *   Vendor    — provider organisation       (e.g. "openai")
 *
 * UI only surfaces the Protocol display name; endpoints and versions are
 * internal implementation details not shown to users.
 *
 * Keep the alias table in sync with the Rust side:
 *   crates/nyro-core/src/protocol/registry.rs::default_protocol_aliases
 */

// ── Protocol enum (canonical short names) ─────────────────────────────────

export type Protocol =
  | "openai-compat"
  | "openai-resps"
  | "anthropic-msgs"
  | "google-genai";

export interface ProtocolMeta {
  id: Protocol;
  /** Human-readable display name shown in the UI. */
  displayName: string;
  /** Default base URL shown as placeholder in the provider form. */
  defaultBaseUrl: string;
}

export const PROTOCOL_TABLE: ProtocolMeta[] = [
  {
    id: "openai-compat",
    displayName: "OpenAI Compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
  },
  {
    id: "openai-resps",
    displayName: "OpenAI Responses",
    defaultBaseUrl: "https://api.openai.com/v1",
  },
  {
    id: "anthropic-msgs",
    displayName: "Anthropic Messages",
    defaultBaseUrl: "https://api.anthropic.com",
  },
  {
    id: "google-genai",
    displayName: "Google Generative AI",
    defaultBaseUrl: "https://generativelanguage.googleapis.com",
  },
];

// ── Alias resolution ───────────────────────────────────────────────────────

/** Maps any known string (old canonical, short alias, legacy brand) → Protocol. */
const PROTOCOL_ALIASES: Record<string, Protocol> = {
  // New canonical short names (idempotent)
  "openai-compat": "openai-compat",
  "openai-resps": "openai-resps",
  "anthropic-msgs": "anthropic-msgs",
  "google-genai": "google-genai",

  // Full names
  "openai-compatible": "openai-compat",
  "openai-responses": "openai-resps",
  "anthropic-messages": "anthropic-msgs",
  "google-generative-ai": "google-genai",

  // Legacy brand names
  openai: "openai-compat",
  openai_responses: "openai-resps",
  responses: "openai-resps",
  anthropic: "anthropic-msgs",
  claude: "anthropic-msgs",
  gemini: "google-genai",
  google: "google-genai",

  // Old canonical endpoint strings (Tier-1 backward compat)
  "openai/chat/v1": "openai-compat",
  "openai/embeddings/v1": "openai-compat",
  "openai/responses/v1": "openai-resps",
  "anthropic/messages/2023-06-01": "anthropic-msgs",
  "google/generate/v1beta": "google-genai",

  // New canonical endpoint strings
  "openai-compat/chat-completions/v1": "openai-compat",
  "openai-compat/embeddings/v1": "openai-compat",
  "openai-resps/responses/v1": "openai-resps",
  "anthropic-msgs/messages/2023-06-01": "anthropic-msgs",
  "google-genai/generate-content/v1beta": "google-genai",
};

/**
 * Resolve any raw protocol string to a canonical `Protocol`, or `null` if unknown.
 *
 * Accepts: new canonical keys (`"openai-compat"`), legacy aliases (`"openai"`),
 * old endpoint canonical strings (`"openai/chat/v1"`), and new endpoint
 * canonical strings (`"openai-compat/chat-completions/v1"`).
 */
export function resolveProtocol(raw: string | null | undefined): Protocol | null {
  if (!raw) return null;
  const key = raw.trim().toLowerCase();
  return PROTOCOL_ALIASES[key] ?? null;
}

/** Return the display name for a protocol string, or `null` if unknown. */
export function protocolDisplayName(raw: string | null | undefined): string | null {
  const protocol = resolveProtocol(raw);
  if (!protocol) return null;
  return PROTOCOL_TABLE.find((p) => p.id === protocol)?.displayName ?? null;
}

/**
 * Legacy shim — resolves a raw string and returns just the display name.
 *
 * Returns `null` when the input is unrecognised so callers can fall back
 * to showing the raw string.
 *
 * @deprecated prefer `protocolDisplayName` for new code.
 */
export function prettyName(raw: string | null | undefined): string | null {
  return protocolDisplayName(raw);
}

// ── ProtocolEndpoint (internal, not shown in UI) ───────────────────────────

export interface ProtocolEndpoint {
  protocol: Protocol;
  name: string;
  version: string;
}

/** Parse a canonical `protocol/name/version` string into a `ProtocolEndpoint`. */
export function parseProtocolEndpoint(raw: string | null | undefined): ProtocolEndpoint | null {
  if (!raw) return null;
  const parts = raw.trim().split("/");
  if (parts.length !== 3 || parts.some((p) => !p)) return null;
  const protocol = resolveProtocol(parts[0]);
  if (!protocol) return null;
  return { protocol, name: parts[1], version: parts[2] };
}

// ── Backward-compat shims for routes.tsx ──────────────────────────────────

/** Returns true when the raw string resolves to an OpenAI-family protocol. */
export function isOpenAiProtocol(raw: string | null | undefined): boolean {
  const p = resolveProtocol(raw);
  return p === "openai-compat" || p === "openai-resps";
}

/**
 * @deprecated — kept for legacy call-sites, use `parseProtocolEndpoint` instead.
 */
export function parseProtocolId(raw: string | null | undefined): { family: string; dialect: string; version: string } | null {
  const ep = parseProtocolEndpoint(raw);
  if (ep) return { family: ep.protocol, dialect: ep.name, version: ep.version };
  // Fallback: try to parse old `family/dialect/version` form verbatim.
  if (!raw) return null;
  const parts = raw.trim().split("/");
  if (parts.length === 3 && parts.every((p) => p.length > 0)) {
    return { family: parts[0], dialect: parts[1], version: parts[2] };
  }
  return null;
}
