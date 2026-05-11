export type ProviderVendor = "openai" | "anthropic" | "gemini";
export type TpmMode = "input_only" | "input_and_output";
export type TokenizerEncoding = "o200k_harmony" | "o200k_base" | "cl100k_base";

export type GatewayAuthStrategy =
  | { kind: "bearer" }
  | { kind: "header_api_key"; header_name: string }
  | { kind: "query_api_key"; parameter_name: string };

export interface GatewayProvider {
  id: string;
  name: string;
  vendor: ProviderVendor;
  base_url: string;
  auth_strategy: GatewayAuthStrategy;
  enabled: boolean;
}

export interface GatewayKey {
  id: string;
  provider_id: string;
  display_name: string | null;
  api_key: string;
  enabled: boolean;
  weight: number | null;
}

export interface GatewayModelRule {
  provider_id: string;
  model: string;
  rpm: number | null;
  rpd: number | null;
  tpm: number | null;
  tpm_mode: TpmMode | null;
  tokenizer_encoding: TokenizerEncoding | null;
  tokenizer_model: string | null;
}

export interface DailyResetConfig {
  timezone: string;
  hour: number;
  minute: number;
}

export interface GatewayProviderBundle {
  provider: GatewayProvider;
  keys: GatewayKey[];
  model_rules: GatewayModelRule[];
  daily_reset: DailyResetConfig;
}

export interface UpstreamRateLimitSummary {
  key_count: number;
  enabled_key_count: number;
  model_rule_count: number;
  has_tpm: boolean;
  has_rpm: boolean;
  has_rpd: boolean;
}

export interface GatewayProviderSummary {
  id: string;
  name: string;
  vendor: ProviderVendor;
  enabled: boolean;
  base_url: string;
  rate_limit: UpstreamRateLimitSummary;
}

export interface RateLimitMetricSnapshot {
  used: number;
  limit: number | null;
  ratio: number | null;
}

export interface KeyModelRuntimeSnapshot {
  key_id: string;
  enabled: boolean;
  available: boolean;
  blocked_reason: string | null;
  active_lease_count: number;
  rpm: RateLimitMetricSnapshot;
  rpd: RateLimitMetricSnapshot;
  tpm: RateLimitMetricSnapshot;
  rpd_window_id: string | null;
}

export interface ProviderModelRuntimeSnapshot {
  model: string;
  matched_rule_model: string | null;
  tpm_mode: TpmMode | null;
  rpm_limit: number | null;
  rpd_limit: number | null;
  tpm_limit: number | null;
  key_count: number;
  enabled_key_count: number;
  available_key_count: number;
  next_cursor: number;
  keys: KeyModelRuntimeSnapshot[];
}

export interface UpstreamRateLimitRuntimeSnapshot {
  captured_at_ms: number;
  models: ProviderModelRuntimeSnapshot[];
}

export interface ProviderRuntimeView {
  summary: GatewayProviderSummary;
  runtime: UpstreamRateLimitRuntimeSnapshot;
}

export interface AdminHealth {
  status: string;
  plane: string;
  bind_addr: string;
  admin_bind_addr: string;
}

type ApiErrorPayload = {
  error?: {
    message?: string;
    type?: string;
  };
};

export class ApiError extends Error {
  readonly status: number;
  readonly type: string | null;

  constructor(status: number, message: string, type: string | null = null) {
    super(message);
    this.status = status;
    this.type = type;
  }
}

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: {
      Accept: "application/json",
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...init?.headers,
    },
  });

  if (!response.ok) {
    let payload: ApiErrorPayload | null = null;
    try {
      payload = (await response.json()) as ApiErrorPayload;
    } catch {
      payload = null;
    }
    throw new ApiError(
      response.status,
      payload?.error?.message ?? `请求失败（${response.status}）`,
      payload?.error?.type ?? null,
    );
  }

  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

export function getAdminHealth() {
  return requestJson<AdminHealth>("/admin/healthz");
}

export function listProviders() {
  return requestJson<GatewayProviderSummary[]>("/admin/providers");
}

export function getProviderBundle(providerId: string) {
  return requestJson<GatewayProviderBundle>(`/admin/providers/${encodeURIComponent(providerId)}`);
}

export function saveProviderBundle(providerId: string, bundle: GatewayProviderBundle) {
  return requestJson<GatewayProviderBundle>(`/admin/providers/${encodeURIComponent(providerId)}`, {
    method: "PUT",
    body: JSON.stringify(bundle),
  });
}

export function deleteProviderBundle(providerId: string) {
  return requestJson<void>(`/admin/providers/${encodeURIComponent(providerId)}`, {
    method: "DELETE",
  });
}

export function listRuntimeProviders() {
  return requestJson<ProviderRuntimeView[]>("/admin/runtime/providers");
}

export function getProviderRuntime(providerId: string) {
  return requestJson<ProviderRuntimeView>(
    `/admin/providers/${encodeURIComponent(providerId)}/runtime`,
  );
}
