export interface Provider {
  id: string;
  name: string;
  vendor?: string | null;
  protocol: string;
  base_url: string;
  default_protocol: string;
  protocol_endpoints: string;
  api_key?: string;
  use_proxy: boolean;
  preset_key?: string | null;
  channel?: string | null;
  models_source?: string | null;
  capabilities_source?: string | null;
  static_models?: string | null;
  is_enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface Route {
  id: string;
  name: string;
  virtual_model: string;
  strategy: RouteStrategy;
  target_provider: string;
  target_model: string;
  access_control: boolean;
  route_type?: "chat" | "embedding";
  cache?: RouteCacheConfig;
  is_enabled: boolean;
  created_at: string;
  targets: RouteTarget[];
}

export type RouteStrategy = "weighted" | "priority";

export interface RouteTarget {
  id: string;
  route_id: string;
  provider_id: string;
  model: string;
  weight: number;
  priority: number;
  created_at: string;
}

export interface ApiKey {
  id: string;
  key: string;
  name: string;
  rpm?: number | null;
  rpd?: number | null;
  tpm?: number | null;
  tpd?: number | null;
  is_enabled: boolean;
  expires_at?: string | null;
  created_at: string;
  updated_at: string;
  route_ids: string[];
}

export interface RequestLog {
  id: string;
  created_at: string;
  ingress_protocol?: string;
  egress_protocol?: string;
  request_model?: string;
  actual_model?: string;
  provider_name?: string;
  status_code?: number;
  duration_ms?: number;
  input_tokens: number;
  output_tokens: number;
  is_stream: boolean;
  is_tool_call: boolean;
  error_message?: string;
}

export interface LogPage {
  items: RequestLog[];
  total: number;
}

export interface GatewayStatus {
  status: string;
  proxy_port: number;
}

export interface StatsOverview {
  total_requests: number;
  total_input_tokens: number;
  total_output_tokens: number;
  avg_duration_ms: number;
  error_count: number;
}

export interface StatsHourly {
  hour: string;
  request_count: number;
  error_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  avg_duration_ms: number;
}

export interface ModelStats {
  model: string;
  request_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  avg_duration_ms: number;
}

export interface ProviderStats {
  provider: string;
  request_count: number;
  error_count: number;
  avg_duration_ms: number;
}

export interface TestResult {
  success: boolean;
  latency_ms: number;
  model?: string;
  error?: string;
}

export interface ModelCapabilities {
  provider: string;
  model_id: string;
  context_window: number;
  embedding_length?: number | null;
  tool_call: boolean;
  reasoning: boolean;
  input_modalities: string[];
  output_modalities: string[];
}

export type ProviderProtocol = "openai" | "anthropic" | "gemini";

export interface ProviderChannelPreset {
  id: string;
  label: {
    zh: string;
    en: string;
  };
  baseUrls: Partial<Record<ProviderProtocol, string>>;
  modelsSource?: string;
  capabilitiesSource?: string;
  apiKey?: string;
  modelsEndpoint?: string;
  staticModels?: string[];
}

export interface ProviderPreset {
  id: string;
  label: {
    zh: string;
    en: string;
  };
  icon?: string;
  defaultProtocol: ProviderProtocol;
  channels?: ProviderChannelPreset[];
}

export interface CreateProvider {
  name: string;
  vendor?: string;
  protocol: string;
  base_url: string;
  default_protocol?: string;
  protocol_endpoints?: string;
  use_proxy?: boolean;
  preset_key?: string;
  channel?: string;
  models_source?: string;
  capabilities_source?: string;
  static_models?: string;
  api_key: string;
}

export interface UpdateProvider {
  name?: string;
  vendor?: string;
  protocol?: string;
  base_url?: string;
  default_protocol?: string;
  protocol_endpoints?: string;
  use_proxy?: boolean;
  preset_key?: string;
  channel?: string;
  models_source?: string;
  capabilities_source?: string;
  static_models?: string;
  api_key?: string;
  is_enabled?: boolean;
}

export interface CreateRoute {
  name: string;
  virtual_model: string;
  strategy?: RouteStrategy;
  target_provider: string;
  target_model: string;
  targets?: CreateRouteTarget[];
  access_control?: boolean;
  route_type?: "chat" | "embedding";
  cache?: RouteCacheConfig | null;
}

export interface UpdateRoute {
  name?: string;
  virtual_model?: string;
  strategy?: RouteStrategy;
  target_provider?: string;
  target_model?: string;
  targets?: UpsertRouteTarget[];
  access_control?: boolean;
  route_type?: "chat" | "embedding";
  cache?: RouteCacheConfig | null;
  is_enabled?: boolean;
}

export interface RouteCacheConfig {
  exact?: RouteExactCacheConfig;
  semantic?: RouteSemanticCacheConfig;
}

export interface RouteExactCacheConfig {
  ttl?: number | null;
}

export interface RouteSemanticCacheConfig {
  ttl?: number | null;
  threshold?: number | null;
}

export interface CacheSettings {
  exact: {
    enabled: boolean;
    default_ttl: number;
    max_entries: number;
    stream_replay_tps: number;
    expose_headers: boolean;
  };
  semantic: {
    enabled: boolean;
    embedding_route: string;
    similarity_threshold: number;
    vector_dimensions: number;
    default_ttl: number;
    max_entries: number;
    stream_replay_tps: number;
    expose_headers: boolean;
  };
}

export interface CreateRouteTarget {
  provider_id: string;
  model: string;
  weight?: number;
  priority?: number;
}

export interface UpsertRouteTarget {
  id?: string;
  provider_id: string;
  model: string;
  weight?: number;
  priority?: number;
}

export interface CreateApiKey {
  name: string;
  rpm?: number;
  rpd?: number;
  tpm?: number;
  tpd?: number;
  expires_at?: string;
  route_ids: string[];
}

export interface UpdateApiKey {
  name?: string;
  rpm?: number;
  rpd?: number;
  tpm?: number;
  tpd?: number;
  is_enabled?: boolean;
  expires_at?: string;
  route_ids?: string[];
}

export interface LogQuery {
  limit?: number;
  offset?: number;
  provider?: string;
  status_min?: number;
  status_max?: number;
}

export interface ExportData {
  version: number;
  providers: ExportProvider[];
  routes: ExportRoute[];
  settings: [string, string][];
}

export interface ExportProvider {
  name: string;
  vendor?: string | null;
  protocol: string;
  base_url: string;
  default_protocol?: string;
  protocol_endpoints?: string;
  use_proxy: boolean;
  preset_key?: string | null;
  channel?: string | null;
  models_source?: string | null;
  capabilities_source?: string | null;
  static_models?: string | null;
  api_key: string;
  is_enabled: boolean;
}

export interface ExportRoute {
  name: string;
  virtual_model: string;
  target_model: string;
  access_control: boolean;
  is_enabled: boolean;
}

export interface ImportResult {
  providers_imported: number;
  routes_imported: number;
  settings_imported: number;
}
