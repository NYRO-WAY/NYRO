import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useRef } from "react";
import { backend, IS_TAURI } from "@/lib/backend";
import type { GatewayStatus, ExportData, ImportResult } from "@/lib/types";
import {
  Copy,
  Check,
  Download,
  Upload,
  Save,
  Loader2,
} from "lucide-react";

export default function SettingsPage() {
  const qc = useQueryClient();
  const [copied, setCopied] = useState(false);
  const [tab, setTab] = useState<"python-openai" | "python-anthropic" | "python-gemini" | "curl">("python-openai");
  const fileRef = useRef<HTMLInputElement>(null);

  const { data: status } = useQuery<GatewayStatus>({
    queryKey: ["gateway-status"],
    queryFn: () => backend("get_gateway_status"),
  });

  const { data: retentionDays } = useQuery<string | null>({
    queryKey: ["setting", "log_retention_days"],
    queryFn: () => backend("get_setting", { key: "log_retention_days" }),
  });

  const [retentionInput, setRetentionInput] = useState<string>("");
  const retentionValue = retentionInput || retentionDays || "30";

  const saveSetting = useMutation({
    mutationFn: (value: string) =>
      backend("set_setting", { key: "log_retention_days", value }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["setting", "log_retention_days"] }),
  });

  const exportMut = useMutation({
    mutationFn: () => backend<ExportData>("export_config"),
    onSuccess: (data) => {
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `nyro-config-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    },
  });

  const importMut = useMutation({
    mutationFn: (data: ExportData) =>
      backend<ImportResult>("import_config", { data }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
      qc.invalidateQueries({ queryKey: ["routes"] });
    },
  });

  function handleImportFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const data = JSON.parse(reader.result as string) as ExportData;
        importMut.mutate(data);
      } catch {
        alert("Invalid JSON file");
      }
    };
    reader.readAsText(file);
    e.target.value = "";
  }

  const baseUrl = `http://127.0.0.1:${status?.proxy_port ?? 18080}`;

  function copyUrl(text: string) {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  const tabs = [
    { key: "python-openai" as const, label: "Python (OpenAI)" },
    { key: "python-anthropic" as const, label: "Python (Anthropic)" },
    { key: "python-gemini" as const, label: "Python (Gemini)" },
    { key: "curl" as const, label: "curl" },
  ];

  const codeExamples: Record<string, string> = {
    "python-openai": `from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}/v1",
    api_key="any-key",  # auth key if configured
)

resp = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(resp.choices[0].message.content)`,
    "python-anthropic": `import anthropic

client = anthropic.Anthropic(
    base_url="${baseUrl}",
    api_key="any-key",  # auth key if configured
)

message = client.messages.create(
    model="claude-sonnet-4-20250514",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello!"}],
)
print(message.content[0].text)`,
    "python-gemini": `import google.generativeai as genai
from google.generativeai.client import configure

# Point the SDK at the Nyro gateway
configure(
    api_key="any-key",
    client_options={"api_endpoint": "127.0.0.1:${status?.proxy_port ?? 18080}"},
    transport="rest",
)

model = genai.GenerativeModel("gemini-2.0-flash")
response = model.generate_content("Hello!")
print(response.text)`,
    curl: `# OpenAI-compatible
curl ${baseUrl}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Hi"}]}'

# Anthropic-compatible
curl ${baseUrl}/v1/messages \\
  -H "Content-Type: application/json" \\
  -H "x-api-key: any-key" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{"model":"claude-sonnet-4-20250514","max_tokens":1024,"messages":[{"role":"user","content":"Hi"}]}'`,
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-slate-900">Settings</h1>
        <p className="mt-1 text-sm text-slate-500">Gateway configuration and quick start guide</p>
      </div>

      {/* Gateway Status */}
      <div className="glass rounded-2xl p-6 space-y-4">
        <h2 className="text-lg font-semibold text-slate-900">Gateway Status</h2>
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">Status</p>
            <p className="mt-1 font-semibold text-green-600">{status?.status ?? "–"}</p>
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">Proxy Port</p>
            <p className="mt-1 font-semibold text-slate-900">{status?.proxy_port ?? "–"}</p>
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">Mode</p>
            <p className="mt-1 font-semibold text-slate-900">{IS_TAURI ? "Desktop" : "Server"}</p>
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">Version</p>
            <p className="mt-1 font-semibold text-slate-900">0.1.0</p>
          </div>
        </div>
      </div>

      {/* Log Retention & Config */}
      <div className="glass rounded-2xl p-6 space-y-5">
        <h2 className="text-lg font-semibold text-slate-900">Configuration</h2>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          {/* Log Retention */}
          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div>
              <p className="text-sm font-medium text-slate-700">Log Retention</p>
              <p className="text-xs text-slate-500">Auto-delete logs older than N days</p>
            </div>
            <div className="flex items-center gap-2">
              <input
                type="number"
                min={1}
                max={365}
                value={retentionValue}
                onChange={(e) => setRetentionInput(e.target.value)}
                className="w-24 rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm outline-none focus:border-slate-400"
              />
              <span className="text-sm text-slate-500">days</span>
              <button
                onClick={() => saveSetting.mutate(retentionValue)}
                disabled={saveSetting.isPending}
                className="ml-auto flex items-center gap-1.5 rounded-lg bg-slate-900 px-3 py-2 text-xs font-medium text-white hover:bg-slate-800 cursor-pointer disabled:opacity-50"
              >
                {saveSetting.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Save className="h-3.5 w-3.5" />
                )}
                Save
              </button>
            </div>
            {saveSetting.isSuccess && (
              <p className="text-xs text-green-600">Saved successfully</p>
            )}
          </div>

          {/* Import / Export */}
          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div>
              <p className="text-sm font-medium text-slate-700">Config Backup</p>
              <p className="text-xs text-slate-500">Export or import providers, routes & settings</p>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={() => exportMut.mutate()}
                disabled={exportMut.isPending}
                className="flex items-center gap-1.5 rounded-lg bg-slate-900 px-3 py-2 text-xs font-medium text-white hover:bg-slate-800 cursor-pointer disabled:opacity-50"
              >
                {exportMut.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                Export
              </button>
              <button
                onClick={() => fileRef.current?.click()}
                disabled={importMut.isPending}
                className="flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs font-medium text-slate-700 hover:bg-slate-50 cursor-pointer disabled:opacity-50"
              >
                {importMut.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Upload className="h-3.5 w-3.5" />
                )}
                Import
              </button>
              <input
                ref={fileRef}
                type="file"
                accept=".json"
                className="hidden"
                onChange={handleImportFile}
              />
            </div>
            {importMut.isSuccess && importMut.data && (
              <p className="text-xs text-green-600">
                Imported: {(importMut.data as ImportResult).providers_imported} providers, {(importMut.data as ImportResult).routes_imported} routes, {(importMut.data as ImportResult).settings_imported} settings
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Quick Start with multi-protocol examples */}
      <div className="glass rounded-2xl p-6 space-y-4">
        <h2 className="text-lg font-semibold text-slate-900">Quick Start</h2>
        <p className="text-sm text-slate-600">
          Point your AI client SDK to this base URL to start proxying requests:
        </p>
        <div className="flex items-center gap-2">
          <code className="flex-1 rounded-xl bg-slate-900 px-4 py-3 text-sm text-green-400 font-mono select-all">
            {baseUrl}/v1
          </code>
          <button
            onClick={() => copyUrl(`${baseUrl}/v1`)}
            className="rounded-xl bg-slate-100 p-3 text-slate-600 hover:bg-slate-200 cursor-pointer transition-colors"
          >
            {copied ? <Check className="h-4 w-4 text-green-600" /> : <Copy className="h-4 w-4" />}
          </button>
        </div>

        <div className="space-y-3 mt-4">
          <p className="text-xs font-semibold text-slate-700 uppercase tracking-wider">Usage Examples</p>
          <div className="flex gap-1 border-b border-slate-200">
            {tabs.map((t) => (
              <button
                key={t.key}
                onClick={() => setTab(t.key)}
                className={`px-3 py-2 text-xs font-medium transition-colors cursor-pointer ${
                  tab === t.key
                    ? "border-b-2 border-slate-900 text-slate-900"
                    : "text-slate-500 hover:text-slate-700"
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <pre className="text-xs text-slate-700 font-mono whitespace-pre-wrap">
              {codeExamples[tab]}
            </pre>
          </div>
        </div>
      </div>

      {/* Setup Guide */}
      <div className="glass rounded-2xl p-6 space-y-4">
        <h2 className="text-lg font-semibold text-slate-900">Setup Guide</h2>
        <div className="space-y-3">
          {[
            { step: 1, title: "Add a Provider", desc: "Go to Providers → Add your OpenAI / Anthropic / Gemini API key" },
            { step: 2, title: "Create a Route", desc: "Go to Routes → Map model patterns (e.g. gpt-4*, claude-*, gemini-*) to a provider" },
            { step: 3, title: "Use the Proxy", desc: "Point your SDK to the base URL above and start making requests" },
          ].map((s) => (
            <div key={s.step} className="flex gap-4 rounded-xl bg-slate-50 p-4">
              <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-slate-900 text-sm font-bold text-white">
                {s.step}
              </div>
              <div>
                <p className="text-sm font-semibold text-slate-900">{s.title}</p>
                <p className="mt-0.5 text-xs text-slate-500">{s.desc}</p>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Supported Protocols */}
      <div className="glass rounded-2xl p-6 space-y-4">
        <h2 className="text-lg font-semibold text-slate-900">Supported Protocols</h2>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          {[
            { name: "OpenAI", endpoint: "/v1/chat/completions", desc: "GPT-4o, o1, o3-mini, DeepSeek..." },
            { name: "Anthropic", endpoint: "/v1/messages", desc: "Claude Sonnet, Haiku, Opus..." },
            { name: "Gemini", endpoint: "/v1beta/models/{model}:*", desc: "Gemini 2.0, 1.5 Pro..." },
          ].map((p) => (
            <div key={p.name} className="rounded-xl bg-slate-50 p-4">
              <p className="text-sm font-semibold text-slate-900">{p.name}</p>
              <code className="mt-1 block text-[11px] text-slate-500 font-mono">{p.endpoint}</code>
              <p className="mt-1 text-xs text-slate-400">{p.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
