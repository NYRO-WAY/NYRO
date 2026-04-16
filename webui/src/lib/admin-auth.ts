export interface AdminConnectionConfig {
  baseUrl: string;
  token: string;
  recentBaseUrls: string[];
}

const STORAGE_KEY = "nyro-admin-connection";
const DEFAULT_RECENT_PATHS = ["http://127.0.0.1:19531", "http://localhost:19531"];

function isBrowser() {
  return typeof window !== "undefined";
}

function normalizeBaseUrl(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "";

  const withProtocol = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
  try {
    const url = new URL(withProtocol);
    return url.origin.replace(/\/$/, "");
  } catch {
    return trimmed.replace(/\/$/, "");
  }
}

function currentOrigin(): string {
  if (!isBrowser()) return "http://127.0.0.1:19531";
  return window.location.origin.replace(/\/$/, "");
}

function uniqueBaseUrls(values: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const normalized = normalizeBaseUrl(value);
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    result.push(normalized);
  }
  return result;
}

export function getDefaultBaseUrl(): string {
  return currentOrigin();
}

export function getAdminConnectionConfig(): AdminConnectionConfig {
  if (!isBrowser()) {
    return {
      baseUrl: "",
      token: "",
      recentBaseUrls: uniqueBaseUrls([currentOrigin(), ...DEFAULT_RECENT_PATHS]),
    };
  }

  const raw = window.localStorage.getItem(STORAGE_KEY);
  if (!raw) {
    return {
      baseUrl: "",
      token: "",
      recentBaseUrls: uniqueBaseUrls([currentOrigin(), ...DEFAULT_RECENT_PATHS]),
    };
  }

  try {
    const parsed = JSON.parse(raw) as Partial<AdminConnectionConfig>;
    return {
      baseUrl: typeof parsed.baseUrl === "string" ? parsed.baseUrl : "",
      token: typeof parsed.token === "string" ? parsed.token : "",
      recentBaseUrls: uniqueBaseUrls([
        ...(Array.isArray(parsed.recentBaseUrls) ? parsed.recentBaseUrls : []),
        currentOrigin(),
        ...DEFAULT_RECENT_PATHS,
      ]),
    };
  } catch {
    return {
      baseUrl: "",
      token: "",
      recentBaseUrls: uniqueBaseUrls([currentOrigin(), ...DEFAULT_RECENT_PATHS]),
    };
  }
}

export function saveAdminConnectionConfig(input: Partial<AdminConnectionConfig>) {
  if (!isBrowser()) return getAdminConnectionConfig();

  const current = getAdminConnectionConfig();
  const next: AdminConnectionConfig = {
    baseUrl: typeof input.baseUrl === "string" ? normalizeBaseUrl(input.baseUrl) : current.baseUrl,
    token: typeof input.token === "string" ? input.token.trim() : current.token,
    recentBaseUrls: uniqueBaseUrls([
      ...(input.recentBaseUrls ?? current.recentBaseUrls),
      typeof input.baseUrl === "string" ? input.baseUrl : current.baseUrl,
      currentOrigin(),
      ...DEFAULT_RECENT_PATHS,
    ]),
  };

  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  window.dispatchEvent(new CustomEvent("nyro-admin-connection-change", { detail: next }));
  return next;
}

export function clearAdminToken() {
  return saveAdminConnectionConfig({ token: "" });
}

export function resolveAdminBaseUrl(baseUrl?: string): string {
  const normalized = normalizeBaseUrl(baseUrl ?? getAdminConnectionConfig().baseUrl);
  return normalized || currentOrigin();
}

export function buildAdminUrl(path: string, baseUrl?: string): string {
  const base = resolveAdminBaseUrl(baseUrl);
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;
  return `${base}${normalizedPath}`;
}

export function buildAdminHeaders(init?: HeadersInit, token?: string): Headers {
  const headers = new Headers(init);
  if (!headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  const effectiveToken = typeof token === "string" ? token.trim() : getAdminConnectionConfig().token.trim();
  if (effectiveToken) {
    headers.set("Authorization", `Bearer ${effectiveToken}`);
  }
  return headers;
}

export class AdminAuthError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "AdminAuthError";
    this.status = status;
  }
}

export async function fetchAdminJson<T>(path: string, init?: RequestInit, baseUrl?: string, token?: string): Promise<T> {
  const response = await fetch(buildAdminUrl(path, baseUrl), {
    ...init,
    headers: buildAdminHeaders(init?.headers, token),
  });

  const text = await response.text();
  const body = text ? JSON.parse(text) : {};

  if (!response.ok) {
    const message =
      (body && typeof body.error === "string" && body.error.trim()) ||
      (body && typeof body.message === "string" && body.message.trim()) ||
      `HTTP ${response.status}`;
    throw new AdminAuthError(message, response.status);
  }

  if (body && typeof body === "object" && "data" in body) {
    return body.data as T;
  }
  return body as T;
}

export async function probeAdminConnection(baseUrl?: string, token?: string) {
  return fetchAdminJson<{ status?: string; proxy_port?: number }>("/api/v1/status", { method: "GET" }, baseUrl, token);
}
