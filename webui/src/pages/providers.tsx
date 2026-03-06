import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { backend } from "@/lib/backend";
import type { Provider, CreateProvider, UpdateProvider, TestResult } from "@/lib/types";
import { Server, Plus, Trash2, CheckCircle, XCircle, Zap, Loader2, Pencil, X, ChevronLeft, ChevronRight } from "lucide-react";
import { useLocale } from "@/lib/i18n";
import { ProviderIcon } from "@/components/ui/provider-icon";

function protocolUrl(protocol: string) {
  switch (protocol) {
    case "anthropic": return "https://api.anthropic.com";
    case "gemini": return "https://generativelanguage.googleapis.com";
    default: return "https://api.openai.com";
  }
}

const emptyCreate: CreateProvider = { name: "", protocol: "openai", base_url: "https://api.openai.com", api_key: "" };
const PAGE_SIZE = 6;

export default function ProvidersPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";

  const qc = useQueryClient();
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<Record<string, TestResult>>({});

  const { data: providers = [], isLoading } = useQuery<Provider[]>({
    queryKey: ["providers"],
    queryFn: () => backend("get_providers"),
  });

  const [form, setForm] = useState<CreateProvider>(emptyCreate);

  const [editForm, setEditForm] = useState<UpdateProvider & { id: string }>({
    id: "", name: "", protocol: "", base_url: "", api_key: "", is_active: true, priority: 0,
  });

  const createMut = useMutation({
    mutationFn: (input: CreateProvider) => backend("create_provider", { input }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
      setShowForm(false);
      setForm(emptyCreate);
    },
  });

  const [editError, setEditError] = useState<string | null>(null);

  const updateMut = useMutation({
    mutationFn: ({ id, ...input }: UpdateProvider & { id: string }) =>
      backend("update_provider", { id, input }),
    onSuccess: () => {
      setEditError(null);
      qc.invalidateQueries({ queryKey: ["providers"] });
      setEditingId(null);
    },
    onError: (err: Error) => {
      setEditError(String(err));
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => backend("delete_provider", { id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["providers"] }),
  });

  async function handleTest(id: string) {
    setTestingId(id);
    try {
      const result = await backend<TestResult>("test_provider", { id });
      setTestResult((prev) => ({ ...prev, [id]: result }));
    } catch (e: unknown) {
      setTestResult((prev) => ({
        ...prev,
        [id]: { success: false, latency_ms: 0, error: String(e) },
      }));
    }
    setTestingId(null);
  }

  function startEdit(p: Provider) {
    setEditingId(p.id);
    setEditForm({
      id: p.id,
      name: p.name,
      protocol: p.protocol,
      base_url: p.base_url,
      api_key: "",
      is_active: p.is_active,
      priority: p.priority,
    });
  }

  const totalPages = Math.max(1, Math.ceil(providers.length / PAGE_SIZE));
  const pagedProviders = providers.slice(page * PAGE_SIZE, page * PAGE_SIZE + PAGE_SIZE);

  useEffect(() => {
    if (page > totalPages - 1) {
      setPage(0);
    }
  }, [page, totalPages]);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">{isZh ? "提供商" : "Providers"}</h1>
          <p className="mt-1 text-sm text-slate-500">
            {isZh ? "管理你的 LLM 提供商连接" : "Manage your LLM provider connections"}
          </p>
        </div>
        <button
          onClick={() => { setShowForm(!showForm); setEditingId(null); }}
          className="flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2.5 text-sm font-medium text-white shadow-md transition-all hover:bg-slate-800 cursor-pointer"
        >
          <Plus className="h-4 w-4" />
          {isZh ? "新增提供商" : "Add Provider"}
        </button>
      </div>

      {/* Create Form */}
      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-slate-900">{isZh ? "新建提供商" : "New Provider"}</h2>
          <div className="grid grid-cols-2 gap-4">
            <input
              placeholder={isZh ? "名称（例如 OpenAI 生产）" : "Name (e.g. OpenAI Production)"}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
            <div className="relative">
              <div className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2">
                <ProviderIcon
                  name={form.name}
                  protocol={form.protocol}
                  baseUrl={form.base_url}
                  size={18}
                />
              </div>
              <select
                value={form.protocol}
                onChange={(e) => setForm({ ...form, protocol: e.target.value, base_url: protocolUrl(e.target.value) })}
                className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 pl-10 text-sm outline-none focus:border-slate-400"
              >
                <option value="openai">OpenAI</option>
                <option value="anthropic">Anthropic</option>
                <option value="gemini">Gemini</option>
              </select>
            </div>
            <input
              placeholder={isZh ? "基础 URL" : "Base URL"}
              value={form.base_url}
              onChange={(e) => setForm({ ...form, base_url: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
            <input
              placeholder="API Key"
              type="password"
              value={form.api_key}
              onChange={(e) => setForm({ ...form, api_key: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
          </div>
          <div className="flex gap-3">
            <button
              onClick={() => createMut.mutate(form)}
              disabled={createMut.isPending || !form.name || !form.api_key}
              className="rounded-xl bg-slate-900 px-5 py-2 text-sm font-medium text-white hover:bg-slate-800 cursor-pointer disabled:opacity-50"
            >
              {createMut.isPending ? (isZh ? "创建中..." : "Creating...") : (isZh ? "创建" : "Create")}
            </button>
            <button
              onClick={() => { setShowForm(false); setForm(emptyCreate); }}
              className="rounded-xl border border-slate-200 px-5 py-2 text-sm font-medium text-slate-600 hover:bg-slate-50 cursor-pointer"
            >
              {isZh ? "取消" : "Cancel"}
            </button>
          </div>
        </div>
      )}

      {/* List */}
      {isLoading ? (
        <div className="text-center text-sm text-slate-500 py-12">{isZh ? "加载中..." : "Loading..."}</div>
      ) : providers.length === 0 ? (
        <div className="glass rounded-2xl p-12 text-center">
          <Server className="mx-auto h-10 w-10 text-slate-400" />
          <p className="mt-3 text-sm text-slate-500">{isZh ? "还没有配置提供商" : "No providers configured yet"}</p>
          <p className="mt-1 text-xs text-slate-400">{isZh ? "添加提供商后开始使用" : "Add a provider to get started"}</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {pagedProviders.map((p) => {
            const tr = testResult[p.id];
            const isEditing = editingId === p.id;

            if (isEditing) {
              return (
                <div key={p.id} className="glass rounded-2xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-900">{isZh ? "编辑提供商" : "Edit Provider"}</h3>
                    <button onClick={() => setEditingId(null)} className="p-1 text-slate-400 hover:text-slate-600 cursor-pointer">
                      <X className="h-4 w-4" />
                    </button>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <input
                      placeholder={isZh ? "名称" : "Name"}
                      value={editForm.name ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <div className="relative">
                      <div className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2">
                        <ProviderIcon
                          name={editForm.name}
                          protocol={editForm.protocol}
                          baseUrl={editForm.base_url}
                          size={18}
                        />
                      </div>
                      <select
                        value={editForm.protocol ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, protocol: e.target.value, base_url: protocolUrl(e.target.value) })}
                        className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 pl-10 text-sm outline-none focus:border-slate-400"
                      >
                        <option value="openai">OpenAI</option>
                        <option value="anthropic">Anthropic</option>
                        <option value="gemini">Gemini</option>
                      </select>
                    </div>
                    <input
                      placeholder={isZh ? "基础 URL" : "Base URL"}
                      value={editForm.base_url ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, base_url: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <input
                      placeholder={isZh ? "API Key（留空则保持不变）" : "API Key (leave empty to keep current)"}
                      type="password"
                      value={editForm.api_key ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, api_key: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <div className="flex items-center gap-3">
                      <label className="text-sm text-slate-600">{isZh ? "启用" : "Active"}</label>
                      <input
                        type="checkbox"
                        checked={editForm.is_active ?? true}
                        onChange={(e) => setEditForm({ ...editForm, is_active: e.target.checked })}
                        className="h-4 w-4 rounded border-slate-300"
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <label className="text-sm text-slate-600">{isZh ? "优先级" : "Priority"}</label>
                      <input
                        type="number"
                        min={0}
                        value={editForm.priority ?? 0}
                        onChange={(e) => setEditForm({ ...editForm, priority: parseInt(e.target.value) || 0 })}
                        className="w-20 rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm outline-none focus:border-slate-400"
                      />
                    </div>
                  </div>
                  <div className="flex gap-3">
                    <button
                      onClick={() => {
                        setEditError(null);
                        const input: UpdateProvider = {
                          name: editForm.name || undefined,
                          protocol: editForm.protocol || undefined,
                          base_url: editForm.base_url || undefined,
                          api_key: editForm.api_key || undefined,
                          is_active: editForm.is_active,
                          priority: editForm.priority,
                        };
                        updateMut.mutate({ id: editForm.id, ...input });
                      }}
                      disabled={updateMut.isPending}
                      className="rounded-xl bg-slate-900 px-5 py-2 text-sm font-medium text-white hover:bg-slate-800 cursor-pointer disabled:opacity-50"
                    >
                      {updateMut.isPending ? (isZh ? "保存中..." : "Saving...") : (isZh ? "保存" : "Save")}
                    </button>
                    <button
                      onClick={() => { setEditingId(null); setEditError(null); }}
                      className="rounded-xl border border-slate-200 px-5 py-2 text-sm font-medium text-slate-600 hover:bg-slate-50 cursor-pointer"
                    >
                      {isZh ? "取消" : "Cancel"}
                    </button>
                  </div>
                  {editError && (
                    <p className="text-xs text-red-600 bg-red-50 rounded-lg px-3 py-2">{editError}</p>
                  )}
                </div>
              );
            }

            return (
              <div key={p.id} className="glass rounded-2xl p-5">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-4">
                    <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-slate-100">
                      <ProviderIcon
                        name={p.name}
                        protocol={p.protocol}
                        baseUrl={p.base_url}
                        size={24}
                      />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-slate-900">{p.name}</span>
                        <span className="protocol-pill inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium uppercase">
                          <ProviderIcon
                            name={p.name}
                            protocol={p.protocol}
                            baseUrl={p.base_url}
                            size={12}
                            className="rounded-sm border-0 bg-transparent"
                          />
                          {p.protocol}
                        </span>
                        {p.is_active ? (
                          <CheckCircle className="h-3.5 w-3.5 text-green-500" />
                        ) : (
                          <XCircle className="h-3.5 w-3.5 text-red-400" />
                        )}
                      </div>
                      <p className="mt-0.5 text-xs text-slate-500">{p.base_url}</p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => handleTest(p.id)}
                      disabled={testingId === p.id}
                      className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-slate-600 border border-slate-200 hover:bg-slate-50 cursor-pointer disabled:opacity-50"
                    >
                      {testingId === p.id ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <Zap className="h-3.5 w-3.5" />
                      )}
                      {isZh ? "测试" : "Test"}
                    </button>
                    <button
                      onClick={() => startEdit(p)}
                      className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-500 cursor-pointer"
                    >
                      <Pencil className="h-4 w-4" />
                    </button>
                    <button
                      onClick={() => deleteMut.mutate(p.id)}
                      className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500 cursor-pointer"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                </div>
                {tr && (
                  <div className={`mt-3 rounded-xl px-4 py-2.5 text-xs ${
                    tr.success ? "bg-green-50 text-green-700" : "bg-red-50 text-red-600"
                  }`}>
                    {tr.success
                      ? `${isZh ? "连接成功" : "Connected"} — ${tr.latency_ms}ms${tr.model ? ` (${tr.model})` : ""}`
                      : `${isZh ? "失败" : "Failed"} — ${tr.error}`
                    }
                  </div>
                )}
              </div>
            );
          })}

          {providers.length > PAGE_SIZE && (
            <div className="flex items-center justify-between px-1 pt-1">
              <span className="text-xs text-slate-500">
                {isZh ? `第 ${page + 1} / ${totalPages} 页` : `Page ${page + 1} of ${totalPages}`}
              </span>
              <div className="flex gap-1">
                <button
                  onClick={() => setPage(Math.max(0, page - 1))}
                  disabled={page === 0}
                  className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 disabled:opacity-30 cursor-pointer"
                >
                  <ChevronLeft className="h-4 w-4" />
                </button>
                <button
                  onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
                  disabled={page >= totalPages - 1}
                  className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 disabled:opacity-30 cursor-pointer"
                >
                  <ChevronRight className="h-4 w-4" />
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
