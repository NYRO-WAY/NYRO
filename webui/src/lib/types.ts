export interface Provider {
  id: string;
  name: string;
  protocol: string;
  base_url: string;
  preset_key?: string | null;
  channel?: string | null;
  models_endpoint?: string | null;
  static_models?: string | null;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface Route {
  id: string;
  name: string;
  ingress_protocol: "openai" | "anthropic" | "gemini";
  virtual_model: string;
  target_provider: string;
  target_model: string;
  access_control: boolean;
  is_active: boolean;
  created_at: string;
}

export interface ApiKey {
  id: string;
  key: string;
  name: string;
  rpm?: number | null;
  tpm?: number | null;
  tpd?: number | null;
  status: "active" | "revoked";
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

export interface CreateProvider {
  name: string;
  protocol: string;
  base_url: string;
  preset_key?: string;
  channel?: string;
  models_endpoint?: string;
  static_models?: string;
  api_key: string;
}

export interface UpdateProvider {
  name?: string;
  protocol?: string;
  base_url?: string;
  preset_key?: string;
  channel?: string;
  models_endpoint?: string;
  static_models?: string;
  api_key?: string;
  is_active?: boolean;
}

export interface CreateRoute {
  name: string;
  ingress_protocol: "openai" | "anthropic" | "gemini";
  virtual_model: string;
  target_provider: string;
  target_model: string;
  access_control?: boolean;
}

export interface UpdateRoute {
  name?: string;
  ingress_protocol?: "openai" | "anthropic" | "gemini";
  virtual_model?: string;
  target_provider?: string;
  target_model?: string;
  access_control?: boolean;
  is_active?: boolean;
}

export interface CreateApiKey {
  name: string;
  rpm?: number;
  tpm?: number;
  tpd?: number;
  expires_at?: string;
  route_ids: string[];
}

export interface UpdateApiKey {
  name?: string;
  rpm?: number;
  tpm?: number;
  tpd?: number;
  status?: "active" | "revoked";
  expires_at?: string;
  route_ids?: string[];
}

export interface LogQuery {
  limit?: number;
  offset?: number;
  provider?: string;
  model?: string;
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
  protocol: string;
  base_url: string;
  preset_key?: string | null;
  channel?: string | null;
  models_endpoint?: string | null;
  static_models?: string | null;
  api_key: string;
  is_active: boolean;
}

export interface ExportRoute {
  name: string;
  ingress_protocol: "openai" | "anthropic" | "gemini";
  virtual_model: string;
  target_provider_name: string;
  target_model: string;
  access_control: boolean;
  is_active: boolean;
}

export interface ImportResult {
  providers_imported: number;
  routes_imported: number;
  settings_imported: number;
}
