import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { backend } from "@/lib/backend";
import type { Route as RouteType, CreateRoute, Provider } from "@/lib/types";
import { Route as RouteIcon, Plus, Trash2, Pencil, X, ChevronLeft, ChevronRight } from "lucide-react";
import { useLocale } from "@/lib/i18n";
import { ProviderIcon } from "@/components/ui/provider-icon";

interface UpdateRoutePayload {
  name?: string;
  match_pattern?: string;
  target_provider?: string;
  target_model?: string;
  fallback_provider?: string;
  fallback_model?: string;
  is_active?: boolean;
  priority?: number;
}

const PAGE_SIZE = 6;

export default function RoutesPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";

  const qc = useQueryClient();
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [page, setPage] = useState(0);

  const { data: routes = [], isLoading } = useQuery<RouteType[]>({
    queryKey: ["routes"],
    queryFn: () => backend("list_routes"),
  });

  const { data: providers = [] } = useQuery<Provider[]>({
    queryKey: ["providers"],
    queryFn: () => backend("get_providers"),
  });

  const createMut = useMutation({
    mutationFn: (input: CreateRoute) => backend("create_route", { input }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["routes"] });
      setShowForm(false);
      setForm(emptyCreate);
    },
  });

  const [editError, setEditError] = useState<string | null>(null);

  const updateMut = useMutation({
    mutationFn: ({ id, ...input }: UpdateRoutePayload & { id: string }) =>
      backend("update_route", { id, input }),
    onSuccess: () => {
      setEditError(null);
      qc.invalidateQueries({ queryKey: ["routes"] });
      setEditingId(null);
    },
    onError: (err: Error) => {
      setEditError(String(err));
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => backend("delete_route", { id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["routes"] }),
  });

  const emptyCreate: CreateRoute = {
    name: "",
    match_pattern: "*",
    target_provider: "",
    target_model: "",
  };

  const [form, setForm] = useState<CreateRoute>(emptyCreate);

  const [editForm, setEditForm] = useState<UpdateRoutePayload & { id: string }>({
    id: "",
    name: "",
    match_pattern: "",
    target_provider: "",
    target_model: "",
    fallback_provider: "",
    fallback_model: "",
    is_active: true,
    priority: 0,
  });

  function startEdit(r: RouteType) {
    setEditingId(r.id);
    setEditForm({
      id: r.id,
      name: r.name,
      match_pattern: r.match_pattern,
      target_provider: r.target_provider,
      target_model: r.target_model,
      fallback_provider: r.fallback_provider ?? "",
      fallback_model: r.fallback_model ?? "",
      is_active: r.is_active,
      priority: r.priority,
    });
  }

  function providerName(id: string) {
    return providers.find((p) => p.id === id)?.name ?? id.slice(0, 8);
  }

  const providerMap = useMemo(
    () => new Map(providers.map((p) => [p.id, p])),
    [providers],
  );

  function providerById(id?: string) {
    if (!id) return undefined;
    return providerMap.get(id);
  }

  const totalPages = Math.max(1, Math.ceil(routes.length / PAGE_SIZE));
  const pagedRoutes = routes.slice(page * PAGE_SIZE, page * PAGE_SIZE + PAGE_SIZE);

  useEffect(() => {
    if (page > totalPages - 1) {
      setPage(0);
    }
  }, [page, totalPages]);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">{isZh ? "路由" : "Routes"}</h1>
          <p className="mt-1 text-sm text-slate-500">{isZh ? "基于模型的路由规则" : "Model-based routing rules"}</p>
        </div>
        <button
          onClick={() => { setShowForm(!showForm); setEditingId(null); }}
          className="flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2.5 text-sm font-medium text-white shadow-md transition-all hover:bg-slate-800 cursor-pointer"
        >
          <Plus className="h-4 w-4" />
          {isZh ? "新增路由" : "Add Route"}
        </button>
      </div>

      {/* Create Form */}
      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-slate-900">{isZh ? "新建路由" : "New Route"}</h2>
          <div className="grid grid-cols-2 gap-4">
            <input
              placeholder={isZh ? "名称" : "Name"}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
            <input
              placeholder={isZh ? "匹配模式（如 gpt-4*、claude-*、*）" : "Match Pattern (e.g. gpt-4*, claude-*, *)"}
              value={form.match_pattern}
              onChange={(e) => setForm({ ...form, match_pattern: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
            <div className="relative">
              <div className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2">
                <ProviderIcon
                  name={providerById(form.target_provider)?.name}
                  protocol={providerById(form.target_provider)?.protocol}
                  baseUrl={providerById(form.target_provider)?.base_url}
                  size={18}
                />
              </div>
              <select
                value={form.target_provider}
                onChange={(e) => setForm({ ...form, target_provider: e.target.value })}
                className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 pl-10 text-sm outline-none focus:border-slate-400"
              >
                <option value="">{isZh ? "选择提供商" : "Select Provider"}</option>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </select>
            </div>
            <input
              placeholder={isZh ? "目标模型（如 gpt-4o，或 * 透传）" : "Target Model (e.g. gpt-4o, or * for passthrough)"}
              value={form.target_model}
              onChange={(e) => setForm({ ...form, target_model: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
          </div>
          <div className="flex gap-3">
            <button
              onClick={() => createMut.mutate(form)}
              disabled={createMut.isPending || !form.name || !form.target_provider}
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
      ) : routes.length === 0 ? (
        <div className="glass rounded-2xl p-12 text-center">
          <RouteIcon className="mx-auto h-10 w-10 text-slate-400" />
          <p className="mt-3 text-sm text-slate-500">{isZh ? "还没有配置路由" : "No routes configured"}</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {pagedRoutes.map((r) => {
            const isEditing = editingId === r.id;
            const targetProvider = providerById(r.target_provider);
            const fallbackProvider = providerById(r.fallback_provider);

            if (isEditing) {
              return (
                <div key={r.id} className="glass rounded-2xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-900">{isZh ? "编辑路由" : "Edit Route"}</h3>
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
                    <input
                      placeholder={isZh ? "匹配模式" : "Match Pattern"}
                      value={editForm.match_pattern ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, match_pattern: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <div className="relative">
                      <div className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2">
                        <ProviderIcon
                          name={providerById(editForm.target_provider)?.name}
                          protocol={providerById(editForm.target_provider)?.protocol}
                          baseUrl={providerById(editForm.target_provider)?.base_url}
                          size={18}
                        />
                      </div>
                      <select
                        value={editForm.target_provider ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, target_provider: e.target.value })}
                        className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 pl-10 text-sm outline-none focus:border-slate-400"
                      >
                        <option value="">{isZh ? "选择提供商" : "Select Provider"}</option>
                        {providers.map((p) => (
                          <option key={p.id} value={p.id}>{p.name}</option>
                        ))}
                      </select>
                    </div>
                    <input
                      placeholder={isZh ? "目标模型（如 gpt-4o，或 * 透传）" : "Target Model (e.g. gpt-4o, or * for passthrough)"}
                      value={editForm.target_model ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, target_model: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <div className="relative">
                      <div className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2">
                        <ProviderIcon
                          name={providerById(editForm.fallback_provider)?.name}
                          protocol={providerById(editForm.fallback_provider)?.protocol}
                          baseUrl={providerById(editForm.fallback_provider)?.base_url}
                          size={18}
                        />
                      </div>
                      <select
                        value={editForm.fallback_provider ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, fallback_provider: e.target.value })}
                        className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 pl-10 text-sm outline-none focus:border-slate-400"
                      >
                        <option value="">{isZh ? "无回退提供商" : "No Fallback Provider"}</option>
                        {providers.map((p) => (
                          <option key={p.id} value={p.id}>{p.name}</option>
                        ))}
                      </select>
                    </div>
                    <input
                      placeholder={isZh ? "回退模型（可选）" : "Fallback Model (optional)"}
                      value={editForm.fallback_model ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, fallback_model: e.target.value })}
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
                        const input: UpdateRoutePayload = {
                          name: editForm.name || undefined,
                          match_pattern: editForm.match_pattern || undefined,
                          target_provider: editForm.target_provider || undefined,
                          target_model: editForm.target_model || undefined,
                          fallback_provider: editForm.fallback_provider || undefined,
                          fallback_model: editForm.fallback_model || undefined,
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
              <div key={r.id} className="glass flex items-center justify-between rounded-2xl p-5">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-900">{r.name}</span>
                    <code className="rounded bg-slate-100 px-2 py-0.5 text-[11px] text-slate-600">
                      {r.match_pattern}
                    </code>
                    {!r.is_active && (
                      <span className="rounded-full bg-red-50 px-2 py-0.5 text-[10px] font-medium text-red-500">
                        {isZh ? "停用" : "Inactive"}
                      </span>
                    )}
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-center gap-2 text-xs">
                    <span className="route-flow-pill inline-flex items-center gap-1.5 rounded-full px-2.5 py-1">
                      <ProviderIcon
                        name={targetProvider?.name}
                        protocol={targetProvider?.protocol}
                        baseUrl={targetProvider?.base_url}
                        size={14}
                        className="rounded-sm border-0 bg-transparent"
                      />
                      <span className="font-medium text-slate-600">{providerName(r.target_provider)}</span>
                      <span className="text-slate-400">→</span>
                      <span className="font-medium text-slate-700">{r.target_model || "*"}</span>
                    </span>
                    {r.fallback_provider && (
                      <span className="route-flow-pill route-flow-pill-fallback inline-flex items-center gap-1.5 rounded-full px-2.5 py-1">
                        <span className="text-[10px] font-medium tracking-wide text-amber-600/85">
                          {isZh ? "回退" : "Fallback"}
                        </span>
                        <ProviderIcon
                          name={fallbackProvider?.name}
                          protocol={fallbackProvider?.protocol}
                          baseUrl={fallbackProvider?.base_url}
                          size={14}
                          className="rounded-sm border-0 bg-transparent"
                        />
                        <span className="font-medium text-slate-600">{providerName(r.fallback_provider)}</span>
                        {r.fallback_model && (
                          <>
                            <span className="text-slate-400">→</span>
                            <span className="font-medium text-slate-700">{r.fallback_model}</span>
                          </>
                        )}
                      </span>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => startEdit(r)}
                    className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-500 cursor-pointer"
                  >
                    <Pencil className="h-4 w-4" />
                  </button>
                  <button
                    onClick={() => deleteMut.mutate(r.id)}
                    className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500 cursor-pointer"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              </div>
            );
          })}

          {routes.length > PAGE_SIZE && (
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
