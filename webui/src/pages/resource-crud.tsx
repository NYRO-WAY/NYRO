import { useMemo, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Header } from "@/components/layout/header";
import { api } from "@/lib/api";
import { SCHEMAS } from "@/lib/resource-schema";
import { ResourceForm } from "@/components/resource-form";
import { Bot, Globe } from "lucide-react";

interface ResourceCrudPageProps {
  title: string;
  resource: string;
}

function getName(item: Record<string, unknown>): string {
  return String(item.name || item.id || "");
}

function cleanFormData(data: Record<string, unknown>): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(data)) {
    if (v === undefined || v === "") continue;
    if (Array.isArray(v) && v.length === 0) continue;
    result[k] = v;
  }
  return result;
}

function normalizePlugins(value: unknown): Array<Record<string, unknown>> {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  return value
    .map((raw) => {
      const plugin = (raw || {}) as Record<string, unknown>;
      const id = String(plugin.id || plugin.name || "");
      if (!id) return null;
      if (seen.has(id)) return null;
      seen.add(id);

      const out: Record<string, unknown> = { id };
      const config = (plugin.config || {}) as Record<string, unknown>;
      const normalizedConfig: Record<string, unknown> = {};
      Object.entries(config).forEach(([k, v]) => {
        if (v === "" || v === undefined || v === null) return;
        if ((k === "from" || k === "to" || k === "key_in") && v === "auto") return;
        normalizedConfig[k] = v;
      });

      if (Object.keys(normalizedConfig).length > 0) {
        out.config = normalizedConfig;
      }
      return out;
    })
    .filter((p): p is Record<string, unknown> => Boolean(p));
}

const AI_PROVIDERS = [
  { id: "openai", label: "OpenAI", icon: "/assets/icons/openai.svg" },
  { id: "anthropic", label: "Anthropic", icon: "/assets/icons/claude.svg" },
  { id: "gemini", label: "Gemini", icon: "/assets/icons/gemini.svg" },
  { id: "ollama", label: "Ollama", icon: "/assets/icons/ollama.svg" },
  { id: "deepseek", label: "DeepSeek", icon: "/assets/icons/deepseek.svg" },
  { id: "kimi", label: "Kimi", icon: "/assets/icons/kimi.svg" },
  { id: "glm", label: "GLM", icon: "/assets/icons/zhipu.svg" },
  { id: "qwen", label: "Qwen", icon: "/assets/icons/qwen.svg" },
  { id: "minimax", label: "Minimax", icon: "/assets/icons/minimax.svg" },
];

const AI_PROVIDER_PATHS: Record<string, string[]> = {
  openai: ["/v1/chat/completions"],
  anthropic: ["/v1/messages"],
  gemini: ["/v1beta/models/*"],
  ollama: ["/api/chat"],
};

const AI_SERVICE_PRESET_URLS: Record<string, string> = {
  openai: "https://api.openai.com",
  anthropic: "https://api.anthropic.com",
  gemini: "https://generativelanguage.googleapis.com",
  ollama: "http://127.0.0.1:11434",
  deepseek: "https://api.deepseek.com",
  kimi: "https://api.moonshot.cn",
  glm: "https://open.bigmodel.cn",
  qwen: "https://dashscope.aliyuncs.com",
  minimax: "https://api.minimaxi.com",
};

const AI_SERVICE_PROVIDER_MAP: Record<string, "openai" | "anthropic" | "gemini" | "ollama"> = {
  openai: "openai",
  anthropic: "anthropic",
  gemini: "gemini",
  ollama: "ollama",
  deepseek: "openai",
  kimi: "openai",
  glm: "openai",
  qwen: "openai",
  minimax: "openai",
};

export default function ResourceCrudPage({ title, resource }: ResourceCrudPageProps) {
  const queryClient = useQueryClient();
  const baseSchema = SCHEMAS[resource];

  const [editorOpen, setEditorOpen] = useState(false);
  const [editingName, setEditingName] = useState("");
  const [formData, setFormData] = useState<Record<string, unknown>>({});
  const [errorMessage, setErrorMessage] = useState("");
  const [detailItem, setDetailItem] = useState<Record<string, unknown> | null>(null);
  
  const [routeMode, setRouteMode] = useState<"http" | "ai">("http");
  const [selectedRouteAiProvider, setSelectedRouteAiProvider] = useState<string>("openai");
  const [serviceType, setServiceType] = useState<"http" | "ai">("http");
  const [serviceProtocol, setServiceProtocol] = useState<string>("http");
  const [serviceTarget, setServiceTarget] = useState<"url" | "backend">("url");

  const servicesQuery = useQuery({
    queryKey: ["resource", "services"],
    queryFn: () => api.list<{ name: string }>("services"),
    enabled: resource === "routes" && editorOpen,
  });

  const listQuery = useQuery({
    queryKey: ["resource", resource],
    queryFn: () => api.list<Record<string, unknown>>(resource),
    refetchInterval: 6000,
  });

  const formFields = useMemo(() => {
    if (!baseSchema) return [];
    let fields = baseSchema.fields.map((f) => ({ ...f }));
    
    if (resource === "routes" && servicesQuery.data?.data?.items) {
      const serviceField = fields.find(f => f.key === "service");
      if (serviceField) {
        const items = servicesQuery.data.data.items;
        serviceField.options = items.map(s => ({
          label: s.name,
          value: s.name
        }));
      }
    }

    if (resource === "services") {
      fields = fields.filter((f) => {
        if (serviceTarget === "url" && f.key === "backend") return false;
        if (serviceTarget === "backend" && f.key === "url") return false;
        if (serviceType === "http" && f.key === "provider") return false;
        return true;
      });
    }
    return fields;
  }, [baseSchema, resource, servicesQuery.data, serviceTarget, serviceType]);

  const items = useMemo(
    () => (Array.isArray(listQuery.data?.data.items) ? listQuery.data?.data.items : []),
    [listQuery.data?.data.items],
  );

  const total = Number(listQuery.data?.data.total || 0);

  const upsertMutation = useMutation({
    mutationFn: async () => {
      const cleaned = cleanFormData(formData);
      if (resource === "routes") {
        if (Array.isArray(cleaned.paths)) {
          cleaned.paths = (cleaned.paths as unknown[])
            .map((p) => String(p || "").trim())
            .filter(Boolean);
        }
        if (Array.isArray(cleaned.hosts)) {
          cleaned.hosts = (cleaned.hosts as unknown[])
            .map((h) => String(h || "").trim())
            .filter(Boolean);
        }
        const options = {
          ...((cleaned.options as Record<string, unknown>) || {}),
          type: routeMode,
          protocol: routeMode === "ai" ? selectedRouteAiProvider : "http",
        };
        cleaned.options = options;

        const normalizedPlugins = normalizePlugins(cleaned.plugins);
        if (normalizedPlugins.length > 0) cleaned.plugins = normalizedPlugins;
        else delete cleaned.plugins;
      }
      if (resource === "services") {
        if (serviceTarget === "url") delete cleaned.backend;
        if (serviceTarget === "backend") delete cleaned.url;

        const options = {
          ...((cleaned.options as Record<string, unknown>) || {}),
          type: serviceType,
          protocol: serviceType === "ai" ? serviceProtocol : "http",
        };
        cleaned.options = options;

        if (serviceType === "ai") {
          cleaned.provider = AI_SERVICE_PROVIDER_MAP[serviceProtocol] || "openai";
          if (serviceTarget === "url" && !cleaned.url) {
            cleaned.url = AI_SERVICE_PRESET_URLS[serviceProtocol] || "";
          }
        } else {
          delete cleaned.provider;
        }
      }
      if (!cleaned.name) throw new Error("name 不能为空");
      if (!editingName) {
        await api.create(resource, cleaned);
      } else {
        await api.update(resource, editingName, cleaned);
      }
    },
    onSuccess: async () => {
      setEditorOpen(false);
      setEditingName("");
      setErrorMessage("");
      await queryClient.invalidateQueries({ queryKey: ["resource", resource] });
    },
    onError: (err: unknown) => {
      setErrorMessage(err instanceof Error ? err.message : "保存失败");
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async (name: string) => {
      await api.remove(resource, name);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["resource", resource] });
    },
  });

  const openCreate = () => {
    setEditingName("");
    setRouteMode("http");
    setSelectedRouteAiProvider("openai");
    setServiceType("http");
    setServiceProtocol("http");
    setServiceTarget("url");
    setFormData(
      baseSchema
        ? {
            ...baseSchema.defaultValues,
            options: { type: "http", protocol: "http" },
          }
        : { name: "" },
    );
    setErrorMessage("");
    setEditorOpen(true);
  };

  const openEdit = (item: Record<string, unknown>) => {
    const name = getName(item);
    setEditingName(name);
    const options = (item.options || {}) as Record<string, unknown>;
    const routeType = String(options.type || options.route_type || "");
    const paths = (item.paths as string[]) || [];
    const isAI =
      routeType === "ai" ||
      paths.some((p) => p.includes("/v1/chat") || p.includes("/v1/completions") || p.includes("/v1/messages"));
    const provider = String(options.protocol || options.ai_client_protocol || "openai");
    setRouteMode(isAI ? "ai" : "http");
    setSelectedRouteAiProvider(provider && provider !== "http" ? provider : "openai");

    if (resource === "services") {
      const detectedType = String(options.type || "");
      const isAiService = detectedType === "ai" || ["openai", "anthropic", "gemini", "ollama"].includes(String(item.provider || ""));
      const protocol = String(options.protocol || item.provider || "openai");
      setServiceType(isAiService ? "ai" : "http");
      setServiceProtocol(isAiService ? protocol : "http");
      setServiceTarget(item.backend ? "backend" : "url");
    }
    
    setFormData({ ...item });
    setErrorMessage("");
    setEditorOpen(true);
  };

  const applyAIPreset = (provider: string) => {
    setSelectedRouteAiProvider(provider);
    setFormData((prev) => ({
      ...prev,
      plugins: [
        { id: "ai-proxy", config: {} },
        ...(((Array.isArray(prev.plugins) ? prev.plugins : []) as Array<Record<string, unknown>>).filter(
          (p) => String(p.id || p.name || "") === "key-auth",
        )),
      ],
      paths: AI_PROVIDER_PATHS[provider] || ["/v1/chat/completions"],
      methods: ["POST", "OPTIONS"],
      options: {
        ...((prev.options as Record<string, unknown>) || {}),
        type: "ai",
        protocol: provider,
      },
    }));
  };

  const applyAIServicePreset = (provider: string) => {
    setServiceType("ai");
    setServiceProtocol(provider);
    setFormData((prev) => ({
      ...prev,
      provider: AI_SERVICE_PROVIDER_MAP[provider] || "openai",
      ...(serviceTarget === "url" ? { url: AI_SERVICE_PRESET_URLS[provider] || "" } : {}),
      options: {
        ...((prev.options as Record<string, unknown>) || {}),
        type: "ai",
        protocol: provider,
      },
    }));
  };

  const columns = baseSchema?.columns || [
    { key: "name", label: "Name" },
  ];

  return (
    <div className="space-y-5">
      <Header
        title={title}
        subtitle={`Resource: /nyro/admin/${resource}`}
        onRefresh={() => void listQuery.refetch()}
        isRefreshing={listQuery.isFetching}
        actions={
          <button
            onClick={openCreate}
            className="cursor-pointer rounded-full bg-slate-900 px-4 py-2 text-xs font-medium text-white transition-opacity hover:opacity-90"
          >
            + 新建
          </button>
        }
      />

      <div className="glass rounded-2xl p-4">
        <div className="mb-3 text-xs text-slate-500">
          total: {total}
        </div>

        {listQuery.isError && (
          <div className="mb-3 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700">
            {(listQuery.error as Error)?.message || "加载失败"}
          </div>
        )}

        <div className="overflow-x-auto rounded-xl border border-white/70 bg-white/55">
          <table className="w-full text-[12px]">
            <thead className="bg-white/70 text-slate-500">
              <tr>
                {columns.map((col) => (
                  <th key={col.key} className="px-3 py-2 text-left font-medium">
                    {col.label}
                  </th>
                ))}
                <th className="px-3 py-2 text-right font-medium">Actions</th>
              </tr>
            </thead>
            <tbody>
              {!listQuery.isLoading && items.length === 0 && (
                <tr>
                  <td className="px-3 py-8 text-center text-slate-500" colSpan={columns.length + 1}>
                    暂无数据
                  </td>
                </tr>
              )}
              {items.map((item) => {
                const name = getName(item);
                return (
                  <tr
                    key={name}
                    className="cursor-pointer border-t border-white/70 text-slate-700 transition-colors hover:bg-white/40"
                    onClick={() => setDetailItem(item)}
                  >
                    {columns.map((col) => (
                      <td key={col.key} className="max-w-[260px] truncate px-3 py-2">
                        {col.render ? col.render(item[col.key], item) : String(item[col.key] ?? "-")}
                      </td>
                    ))}
                    <td className="px-3 py-2 text-right">
                      <div className="inline-flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
                        <button
                          onClick={() => openEdit(item)}
                          className="cursor-pointer rounded-md border border-slate-200 bg-white px-2 py-1 text-xs hover:bg-slate-50"
                        >
                          编辑
                        </button>
                        <button
                          onClick={() => {
                            if (!name) return;
                            if (!window.confirm(`确认删除 ${name} ?`)) return;
                            deleteMutation.mutate(name);
                          }}
                          className="cursor-pointer rounded-md border border-rose-200 bg-rose-50 px-2 py-1 text-xs text-rose-600 hover:bg-rose-100"
                        >
                          删除
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>

      {detailItem && (
        <div className="fixed inset-0 z-50 flex justify-end" onClick={() => setDetailItem(null)}>
          <div className="absolute inset-0 bg-black/20 backdrop-blur-sm" />
          <div
            className="relative z-10 flex h-full w-full max-w-lg flex-col bg-white/95 shadow-2xl backdrop-blur-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-slate-100 px-5 py-4">
              <h3 className="text-sm font-semibold text-slate-800">
                {getName(detailItem)}
              </h3>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => { openEdit(detailItem); setDetailItem(null); }}
                  className="cursor-pointer rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs hover:bg-slate-50"
                >
                  编辑
                </button>
                <button
                  onClick={() => setDetailItem(null)}
                  className="cursor-pointer rounded-lg px-2 py-1.5 text-xs text-slate-400 hover:text-slate-700"
                >
                  关闭
                </button>
              </div>
            </div>
            <div className="flex-1 overflow-y-auto px-5 py-4">
              <div className="space-y-3">
                {(baseSchema?.fields || [{ key: "name", label: "Name", type: "text" as const }]).map(
                  (field) => {
                    const val = detailItem[field.key];
                    return (
                      <div key={field.key}>
                        <div className="mb-0.5 text-[11px] font-medium uppercase tracking-wide text-slate-400">
                          {field.label}
                        </div>
                        <div className="rounded-lg border border-slate-100 bg-slate-50/60 px-3 py-2 text-xs text-slate-700 break-all">
                          {renderDetailValue(field.type, val)}
                        </div>
                      </div>
                    );
                  },
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {editorOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <div className="absolute inset-0 bg-black/20 backdrop-blur-sm" onClick={() => setEditorOpen(false)} />
          <div className="relative z-10 w-full max-w-2xl rounded-2xl border border-white/60 bg-white/95 shadow-2xl backdrop-blur-xl flex flex-col max-h-[90vh]">
            <div className="flex items-center justify-between border-b border-slate-100 px-5 py-4">
              <h3 className="text-sm font-semibold text-slate-800">
                {editingName ? `编辑 ${editingName}` : `新建 ${resource}`}
              </h3>
              <button
                onClick={() => setEditorOpen(false)}
                className="cursor-pointer rounded-md px-2 py-1 text-xs text-slate-500 hover:bg-slate-100"
              >
                ✕
              </button>
            </div>

            <div className="flex-1 overflow-y-auto px-5 py-4">
              {baseSchema ? (
                <ResourceForm
                  fields={formFields}
                  value={formData}
                  onChange={setFormData}
                  editingName={editingName || undefined}
                  extraContent={resource === "routes" ? (
                    <div className="rounded-xl border border-slate-100 bg-slate-50/50 p-3">
                      <div className="mb-3 flex gap-4 border-b border-slate-200 pb-2">
                         <label className="flex cursor-pointer items-center gap-2">
                           <input
                             type="radio" 
                             name="routeMode" 
                             checked={routeMode === "http"} 
                             onChange={() => {
                               setRouteMode("http");
                               setFormData((prev) => ({
                                 ...prev,
                                 options: {
                                   ...((prev.options as Record<string, unknown>) || {}),
                                   type: "http",
                                   protocol: "http",
                                 },
                               }));
                             }}
                             className="accent-slate-900"
                           />
                           <span className="flex items-center gap-1.5 text-xs font-medium text-slate-700">
                             <Globe className="h-3.5 w-3.5" /> HTTP Route
                           </span>
                         </label>
                         <label className="flex cursor-pointer items-center gap-2">
                           <input
                             type="radio" 
                             name="routeMode" 
                             checked={routeMode === "ai"} 
                             onChange={() => {
                               setRouteMode("ai");
                               applyAIPreset(selectedRouteAiProvider || "openai");
                             }}
                             className="accent-slate-900"
                           />
                           <span className="flex items-center gap-1.5 text-xs font-medium text-slate-700">
                             <Bot className="h-3.5 w-3.5" /> AI Route
                           </span>
                         </label>
                      </div>
                      
                      {routeMode === "ai" && (
                        <div>
                          <p className="mb-2 text-[11px] text-slate-400">选择厂商模板自动填充 Paths, Methods 和 Plugins</p>
                          <div className="flex flex-wrap gap-2">
                            {AI_PROVIDERS.map(p => (
                              <button
                                key={p.id}
                                onClick={() => applyAIPreset(p.id)}
                                className={[
                                  "flex items-center gap-1.5 whitespace-nowrap rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors",
                                  selectedRouteAiProvider === p.id
                                    ? "border-indigo-400 bg-indigo-50 text-indigo-700"
                                    : "border-slate-200 bg-white text-slate-600 hover:border-indigo-300 hover:text-indigo-600",
                                ].join(" ")}
                              >
                                <img
                                  src={p.icon}
                                  alt={p.label}
                                  className="h-3.5 w-3.5"
                                />
                                {p.label}
                              </button>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  ) : resource === "services" ? (
                    <div className="rounded-xl border border-slate-100 bg-slate-50/50 p-3">
                      <div className="mb-3 flex gap-4 border-b border-slate-200 pb-2">
                        <label className="flex cursor-pointer items-center gap-2">
                          <input
                            type="radio"
                            name="serviceType"
                            checked={serviceType === "http"}
                            onChange={() => {
                              setServiceType("http");
                              setServiceProtocol("http");
                              setFormData((prev) => ({
                                ...prev,
                                options: {
                                  ...((prev.options as Record<string, unknown>) || {}),
                                  type: "http",
                                  protocol: "http",
                                },
                              }));
                            }}
                            className="accent-slate-900"
                          />
                          <span className="flex items-center gap-1.5 text-xs font-medium text-slate-700">
                            <Globe className="h-3.5 w-3.5" /> HTTP Service
                          </span>
                        </label>
                        <label className="flex cursor-pointer items-center gap-2">
                          <input
                            type="radio"
                            name="serviceType"
                            checked={serviceType === "ai"}
                            onChange={() => applyAIServicePreset(serviceProtocol === "http" ? "openai" : serviceProtocol)}
                            className="accent-slate-900"
                          />
                          <span className="flex items-center gap-1.5 text-xs font-medium text-slate-700">
                            <Bot className="h-3.5 w-3.5" /> AI Service
                          </span>
                        </label>
                      </div>

                      <div className="mb-3 flex gap-2">
                        <button
                          type="button"
                          onClick={() => {
                            setServiceTarget("url");
                            setFormData((prev) => ({ ...prev, backend: "" }));
                          }}
                          className={[
                            "rounded-lg border px-3 py-1.5 text-xs font-medium",
                            serviceTarget === "url"
                              ? "border-slate-900 bg-slate-900 text-white"
                              : "border-slate-200 bg-white text-slate-600 hover:bg-slate-50",
                          ].join(" ")}
                        >
                          URL
                        </button>
                        <button
                          type="button"
                          onClick={() => {
                            setServiceTarget("backend");
                            setFormData((prev) => ({ ...prev, url: "" }));
                          }}
                          className={[
                            "rounded-lg border px-3 py-1.5 text-xs font-medium",
                            serviceTarget === "backend"
                              ? "border-slate-900 bg-slate-900 text-white"
                              : "border-slate-200 bg-white text-slate-600 hover:bg-slate-50",
                          ].join(" ")}
                        >
                          Backend
                        </button>
                      </div>

                      {serviceType === "ai" && (
                        <div>
                          <p className="mb-2 text-[11px] text-slate-400">选择 AI 厂商快捷填充 provider / options / URL</p>
                          <div className="flex flex-wrap gap-2">
                            {AI_PROVIDERS.map((p) => (
                              <button
                                key={p.id}
                                type="button"
                                onClick={() => applyAIServicePreset(p.id)}
                                className={[
                                  "flex items-center gap-1.5 whitespace-nowrap rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors",
                                  serviceProtocol === p.id
                                    ? "border-indigo-400 bg-indigo-50 text-indigo-700"
                                    : "border-slate-200 bg-white text-slate-600 hover:border-indigo-300 hover:text-indigo-600",
                                ].join(" ")}
                              >
                                <img src={p.icon} alt={p.label} className="h-3.5 w-3.5" />
                                {p.label}
                              </button>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  ) : undefined}
                />
              ) : (
                <textarea
                  value={JSON.stringify(formData, null, 2)}
                  onChange={(e) => {
                    try { setFormData(JSON.parse(e.target.value)); } catch { /* ignore */ }
                  }}
                  rows={18}
                  className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 font-mono text-xs outline-none focus:border-slate-400"
                />
              )}
            </div>

            {errorMessage && (
              <div className="border-t border-slate-100 px-5 py-2">
                <p className="text-xs text-rose-600">{errorMessage}</p>
              </div>
            )}

            <div className="flex justify-end gap-2 border-t border-slate-100 px-5 py-3">
              <button
                onClick={() => setEditorOpen(false)}
                className="cursor-pointer rounded-lg border border-slate-200 bg-white px-4 py-2 text-xs transition-colors hover:bg-slate-50"
              >
                取消
              </button>
              <button
                onClick={() => upsertMutation.mutate()}
                className="cursor-pointer rounded-lg bg-slate-900 px-4 py-2 text-xs text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                disabled={upsertMutation.isPending}
              >
                {upsertMutation.isPending ? "保存中..." : "保存"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function renderDetailValue(type: string, val: unknown): ReactNode {
  if (val === undefined || val === null || val === "") return <span className="text-slate-400">-</span>;

  if (type === "tags") {
    if (!Array.isArray(val)) return String(val);
    return (
      <div className="flex flex-wrap gap-1">
        {(val as string[]).map((tag, i) => (
          <span key={`${tag}-${i}`} className="rounded-md bg-slate-200/60 px-1.5 py-0.5 text-[11px]">
            {tag}
          </span>
        ))}
      </div>
    );
  }

  if (type === "path-list") {
    if (!Array.isArray(val)) return String(val);
    return (
      <div className="space-y-1">
        {(val as string[]).map((path, i) => (
          <div key={`${path}-${i}`} className="rounded bg-slate-100 px-1.5 py-0.5 text-[11px]">
            {path}
          </div>
        ))}
      </div>
    );
  }
  
  if (type === "multi-select") {
     if (!Array.isArray(val)) return String(val);
     return (
       <div className="flex flex-wrap gap-1">
         {(val as string[]).map((v, i) => (
           <span key={i} className="rounded-md bg-slate-800 text-white px-1.5 py-0.5 text-[10px]">
             {v}
           </span>
         ))}
       </div>
     );
  }

  if (type === "endpoints") {
    if (!Array.isArray(val)) return String(val);
    return (
      <div className="space-y-1">
        {(val as Array<{ address: string; port: number; weight: number }>).map((ep, i) => (
          <div key={i} className="flex gap-2 text-[11px]">
            <span>{ep.address}</span>
            <span className="text-slate-400">:</span>
            <span>{ep.port}</span>
            <span className="text-slate-400">w={ep.weight}</span>
          </div>
        ))}
      </div>
    );
  }

  if (type === "plugins") {
    if (!Array.isArray(val) || val.length === 0) return <span className="text-slate-400">-</span>;
    return (
      <div className="space-y-1">
        {(val as Array<{ id?: string; name?: string }>).map((p, i) => (
          <span key={i} className="mr-1.5 inline-block rounded-md bg-indigo-50 px-1.5 py-0.5 text-[11px] text-indigo-600">
            {p.id || p.name}
          </span>
        ))}
      </div>
    );
  }

  if (type === "json" || type === "credentials") {
    return (
      <pre className="whitespace-pre-wrap font-mono text-[11px]">
        {typeof val === "object" ? JSON.stringify(val, null, 2) : String(val)}
      </pre>
    );
  }

  if (type === "textarea") {
    return <pre className="whitespace-pre-wrap font-mono text-[11px]">{String(val)}</pre>;
  }

  return String(val);
}
