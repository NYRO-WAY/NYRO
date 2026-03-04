export interface Provider {
  id: string;
  name: string;
  protocol: string;
  base_url: string;
  is_active: boolean;
  priority: number;
  created_at: string;
  updated_at: string;
}

export interface Route {
  id: string;
  name: string;
  match_pattern: string;
  target_provider: string;
  target_model: string;
  fallback_provider?: string;
  fallback_model?: string;
  is_active: boolean;
  priority: number;
  created_at: string;
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
  api_key: string;
}

export interface UpdateProvider {
  name?: string;
  protocol?: string;
  base_url?: string;
  api_key?: string;
  is_active?: boolean;
  priority?: number;
}

export interface CreateRoute {
  name: string;
  match_pattern: string;
  target_provider: string;
  target_model: string;
  fallback_provider?: string;
  fallback_model?: string;
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
  api_key: string;
  is_active: boolean;
  priority: number;
}

export interface ExportRoute {
  name: string;
  match_pattern: string;
  target_provider_name: string;
  target_model: string;
  fallback_provider_name?: string;
  fallback_model?: string;
  is_active: boolean;
  priority: number;
}

export interface ImportResult {
  providers_imported: number;
  routes_imported: number;
  settings_imported: number;
}
