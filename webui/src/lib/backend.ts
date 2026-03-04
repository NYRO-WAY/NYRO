const IS_TAURI = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invokeIPC<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function invokeHTTP<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const mapping = resolveHTTP(cmd, args);
  const init: RequestInit = { method: mapping.method };
  if (mapping.body) {
    init.headers = { "Content-Type": "application/json" };
    init.body = JSON.stringify(mapping.body);
  }
  const resp = await fetch(mapping.url, init);
  if (!resp.ok) {
    const body = await resp.json().catch(() => ({}));
    throw new Error(body.error || `HTTP ${resp.status}`);
  }
  const text = await resp.text();
  if (!text) return {} as T;
  const json = JSON.parse(text);
  return json.data ?? json;
}

interface HTTPMapping {
  method: string;
  url: string;
  body?: Record<string, unknown>;
}

function resolveHTTP(cmd: string, args?: Record<string, unknown>): HTTPMapping {
  const base = "/api/v1";
  switch (cmd) {
    case "get_providers":
      return { method: "GET", url: `${base}/providers` };
    case "get_provider":
      return { method: "GET", url: `${base}/providers/${args?.id}` };
    case "create_provider":
      return { method: "POST", url: `${base}/providers`, body: args?.input as Record<string, unknown> };
    case "update_provider":
      return { method: "PUT", url: `${base}/providers/${args?.id}`, body: args?.input as Record<string, unknown> };
    case "delete_provider":
      return { method: "DELETE", url: `${base}/providers/${args?.id}` };
    case "test_provider":
      return { method: "GET", url: `${base}/providers/${args?.id}/test` };

    case "list_routes":
      return { method: "GET", url: `${base}/routes` };
    case "create_route":
      return { method: "POST", url: `${base}/routes`, body: args?.input as Record<string, unknown> };
    case "update_route":
      return { method: "PUT", url: `${base}/routes/${args?.id}`, body: args?.input as Record<string, unknown> };
    case "delete_route":
      return { method: "DELETE", url: `${base}/routes/${args?.id}` };

    case "query_logs": {
      const q = (args?.query ?? {}) as Record<string, unknown>;
      const params = new URLSearchParams();
      for (const [k, v] of Object.entries(q)) {
        if (v != null) params.set(k, String(v));
      }
      const qs = params.toString();
      return { method: "GET", url: `${base}/logs${qs ? "?" + qs : ""}` };
    }

    case "get_stats_overview":
      return { method: "GET", url: `${base}/stats/overview` };
    case "get_stats_hourly": {
      const hours = args?.hours ?? 24;
      return { method: "GET", url: `${base}/stats/hourly?hours=${hours}` };
    }
    case "get_stats_by_model":
      return { method: "GET", url: `${base}/stats/models` };
    case "get_stats_by_provider":
      return { method: "GET", url: `${base}/stats/providers` };

    case "get_setting":
      return { method: "GET", url: `${base}/settings/${args?.key}` };
    case "set_setting":
      return { method: "PUT", url: `${base}/settings/${args?.key}`, body: { value: args?.value } };

    case "get_gateway_status":
      return { method: "GET", url: `${base}/status` };

    case "export_config":
      return { method: "GET", url: `${base}/config/export` };
    case "import_config":
      return { method: "POST", url: `${base}/config/import`, body: args?.data as Record<string, unknown> };

    default:
      return { method: "POST", url: `${base}/${cmd}`, body: args };
  }
}

export const backend = IS_TAURI ? invokeIPC : invokeHTTP;
export { IS_TAURI };
