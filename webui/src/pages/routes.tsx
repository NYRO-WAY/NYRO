import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { backend } from "@/lib/backend";
import type { Route as RouteType, CreateRoute, Provider } from "@/lib/types";
import { Route as RouteIcon, Plus, Trash2, Pencil, X, ChevronLeft, ChevronRight } from "lucide-react";
import { useLocale } from "@/lib/i18n";
import { ProviderIcon } from "@/components/ui/provider-icon";
import { NyroButton } from "@/components/ui/nyro-button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

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
const NONE_OPTION = "__none__";

function FieldLabel({ children }: { children: string }) {
  return <label className="ml-1 text-xs leading-none font-normal text-slate-900">{children}</label>;
}

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
  const { data: createTargetModels = [] } = useQuery<string[]>({
    queryKey: ["provider-models", form.target_provider],
    queryFn: () => backend("get_provider_models", { id: form.target_provider }),
    enabled: !!providers.find((provider) =>
      provider.id === form.target_provider && (provider.models_endpoint || provider.static_models),
    ),
    staleTime: 60_000,
  });
  const { data: editTargetModels = [] } = useQuery<string[]>({
    queryKey: ["provider-models", editForm.target_provider],
    queryFn: () => backend("get_provider_models", { id: editForm.target_provider }),
    enabled: !!providers.find((provider) =>
      provider.id === editForm.target_provider && (provider.models_endpoint || provider.static_models),
    ),
    staleTime: 60_000,
  });
  const { data: editFallbackModels = [] } = useQuery<string[]>({
    queryKey: ["provider-models", editForm.fallback_provider],
    queryFn: () => backend("get_provider_models", { id: editForm.fallback_provider }),
    enabled: !!providers.find((provider) =>
      provider.id === editForm.fallback_provider && (provider.models_endpoint || provider.static_models),
    ),
    staleTime: 60_000,
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
  const providerOptions = useMemo(
    () => providers.map((p) => ({ value: p.id, label: p.name, provider: p })),
    [providers],
  );
  const createTargetProvider = providerById(form.target_provider);
  const editTargetProvider = providerById(editForm.target_provider);
  const editFallbackProviderRecord = providerById(editForm.fallback_provider);

  function providerById(id?: string) {
    if (!id) return undefined;
    return providerMap.get(id);
  }

  function hasProviderModelOptions(provider?: Provider) {
    return Boolean(provider?.models_endpoint || provider?.static_models);
  }

  function withCurrentModel(options: string[], current?: string) {
    if (!current || options.includes(current)) return options;
    return [current, ...options];
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
        <NyroButton
          onClick={() => { setShowForm(!showForm); setEditingId(null); }}
          variant="primary"
          className="flex items-center gap-2"
        >
          <Plus className="h-4 w-4" />
          {isZh ? "新增路由" : "Add Route"}
        </NyroButton>
      </div>

      {/* Create Form */}
      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-slate-900">{isZh ? "新建路由" : "New Route"}</h2>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
              <Input
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                placeholder={isZh ? "输入路由名称" : "Enter route name"}
              />
            </div>
            <div className="space-y-2">
              <FieldLabel>{isZh ? "匹配模式" : "Match Pattern"}</FieldLabel>
              <Input
                value={form.match_pattern}
                onChange={(e) => setForm({ ...form, match_pattern: e.target.value })}
                placeholder={isZh ? "如 gpt-4*、claude-*、*" : "e.g. gpt-4*, claude-*, *"}
              />
            </div>
            <div className="space-y-2">
              <FieldLabel>{isZh ? "目标提供商" : "Target Provider"}</FieldLabel>
              <Select
                value={form.target_provider || undefined}
                onValueChange={(value) => setForm({ ...form, target_provider: value })}
              >
                <SelectTrigger>
                  <SelectValue placeholder={isZh ? "选择提供商" : "Select provider"} />
                </SelectTrigger>
                <SelectContent>
                  {providerOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      <span className="flex items-center gap-2">
                        <ProviderIcon
                          name={option.provider.name}
                          protocol={option.provider.protocol}
                          baseUrl={option.provider.base_url}
                          size={16}
                        />
                        <span>{option.label}</span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {hasProviderModelOptions(createTargetProvider) ? (
              <div className="space-y-2">
                <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                <Select
                  value={form.target_model || NONE_OPTION}
                  onValueChange={(value) => setForm({ ...form, target_model: value === NONE_OPTION ? "" : value })}
                >
                  <SelectTrigger>
                    <SelectValue placeholder={isZh ? "选择目标模型" : "Select target model"} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NONE_OPTION}>{isZh ? "请选择" : "Select one"}</SelectItem>
                    {withCurrentModel(createTargetModels, form.target_model).map((model) => (
                      <SelectItem key={model} value={model}>
                        {model}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ) : (
              <div className="space-y-2">
                <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                <Input
                  value={form.target_model}
                  onChange={(e) => setForm({ ...form, target_model: e.target.value })}
                  placeholder={isZh ? "如 gpt-4o 或 * 透传" : "e.g. gpt-4o or * passthrough"}
                />
              </div>
            )}
          </div>
          <div className="flex gap-3">
            <NyroButton
              onClick={() => createMut.mutate(form)}
              disabled={createMut.isPending || !form.name || !form.target_provider}
              variant="primary"
            >
              {createMut.isPending ? (isZh ? "创建中..." : "Creating...") : (isZh ? "创建" : "Create")}
            </NyroButton>
            <NyroButton
              onClick={() => { setShowForm(false); setForm(emptyCreate); }}
              variant="secondary"
            >
              {isZh ? "取消" : "Cancel"}
            </NyroButton>
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
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
                      <Input
                        value={editForm.name ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                        placeholder={isZh ? "输入路由名称" : "Enter route name"}
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "匹配模式" : "Match Pattern"}</FieldLabel>
                      <Input
                        value={editForm.match_pattern ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, match_pattern: e.target.value })}
                        placeholder={isZh ? "如 gpt-4*、claude-*、*" : "e.g. gpt-4*, claude-*, *"}
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "目标提供商" : "Target Provider"}</FieldLabel>
                      <Select
                        value={editForm.target_provider || undefined}
                        onValueChange={(value) => setEditForm({ ...editForm, target_provider: value })}
                      >
                        <SelectTrigger>
                          <SelectValue placeholder={isZh ? "选择提供商" : "Select provider"} />
                        </SelectTrigger>
                        <SelectContent>
                          {providerOptions.map((option) => (
                            <SelectItem key={option.value} value={option.value}>
                              <span className="flex items-center gap-2">
                                <ProviderIcon
                                  name={option.provider.name}
                                  protocol={option.provider.protocol}
                                  baseUrl={option.provider.base_url}
                                  size={16}
                                />
                                <span>{option.label}</span>
                              </span>
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    {hasProviderModelOptions(editTargetProvider) ? (
                      <div className="space-y-2">
                        <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                        <Select
                          value={editForm.target_model || NONE_OPTION}
                          onValueChange={(value) =>
                            setEditForm({ ...editForm, target_model: value === NONE_OPTION ? "" : value })
                          }
                        >
                          <SelectTrigger>
                            <SelectValue placeholder={isZh ? "选择目标模型" : "Select target model"} />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value={NONE_OPTION}>{isZh ? "请选择" : "Select one"}</SelectItem>
                            {withCurrentModel(editTargetModels, editForm.target_model ?? undefined).map((model) => (
                              <SelectItem key={model} value={model}>
                                {model}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                    ) : (
                      <div className="space-y-2">
                        <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                        <Input
                          value={editForm.target_model ?? ""}
                          onChange={(e) => setEditForm({ ...editForm, target_model: e.target.value })}
                          placeholder={isZh ? "如 gpt-4o 或 * 透传" : "e.g. gpt-4o or * passthrough"}
                        />
                      </div>
                    )}
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "回退提供商" : "Fallback Provider"}</FieldLabel>
                      <Select
                        value={editForm.fallback_provider || NONE_OPTION}
                        onValueChange={(value) =>
                          setEditForm({ ...editForm, fallback_provider: value === NONE_OPTION ? "" : value })
                        }
                      >
                        <SelectTrigger>
                          <SelectValue placeholder={isZh ? "无回退提供商" : "No fallback provider"} />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={NONE_OPTION}>{isZh ? "无" : "None"}</SelectItem>
                          {providerOptions.map((option) => (
                            <SelectItem key={option.value} value={option.value}>
                              <span className="flex items-center gap-2">
                                <ProviderIcon
                                  name={option.provider.name}
                                  protocol={option.provider.protocol}
                                  baseUrl={option.provider.base_url}
                                  size={16}
                                />
                                <span>{option.label}</span>
                              </span>
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    {hasProviderModelOptions(editFallbackProviderRecord) ? (
                      <div className="space-y-2">
                        <FieldLabel>{isZh ? "回退模型" : "Fallback Model"}</FieldLabel>
                        <Select
                          value={editForm.fallback_model || NONE_OPTION}
                          onValueChange={(value) =>
                            setEditForm({ ...editForm, fallback_model: value === NONE_OPTION ? "" : value })
                          }
                        >
                          <SelectTrigger>
                            <SelectValue placeholder={isZh ? "选择回退模型" : "Select fallback model"} />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value={NONE_OPTION}>{isZh ? "无" : "None"}</SelectItem>
                            {withCurrentModel(editFallbackModels, editForm.fallback_model ?? undefined).map((model) => (
                              <SelectItem key={model} value={model}>
                                {model}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                    ) : (
                      <div className="space-y-2">
                        <FieldLabel>{isZh ? "回退模型" : "Fallback Model"}</FieldLabel>
                        <Input
                          value={editForm.fallback_model ?? ""}
                          onChange={(e) => setEditForm({ ...editForm, fallback_model: e.target.value })}
                          placeholder={isZh ? "可选" : "Optional"}
                        />
                      </div>
                    )}
                    <div className="flex items-center gap-3">
                      <label className="text-sm text-slate-600">{isZh ? "启用" : "Active"}</label>
                      <input
                        type="checkbox"
                        checked={editForm.is_active ?? true}
                        onChange={(e) => setEditForm({ ...editForm, is_active: e.target.checked })}
                        className="h-4 w-4 rounded border-slate-300"
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "优先级" : "Priority"}</FieldLabel>
                      <Input
                        type="text"
                        inputMode="numeric"
                        pattern="[0-9]*"
                        value={String(editForm.priority ?? 0)}
                        onChange={(e) => {
                          const nextValue = e.target.value.replace(/\D+/g, "");
                          setEditForm({ ...editForm, priority: Number.parseInt(nextValue || "0", 10) });
                        }}
                      />
                    </div>
                  </div>
                  <div className="flex gap-3">
                    <NyroButton
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
                      variant="primary"
                    >
                      {updateMut.isPending ? (isZh ? "保存中..." : "Saving...") : (isZh ? "保存" : "Save")}
                    </NyroButton>
                    <NyroButton
                      onClick={() => { setEditingId(null); setEditError(null); }}
                      variant="secondary"
                    >
                      {isZh ? "取消" : "Cancel"}
                    </NyroButton>
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
                <NyroButton
                  onClick={() => setPage(Math.max(0, page - 1))}
                  disabled={page === 0}
                  variant="icon"
                >
                  <ChevronLeft className="h-4 w-4" />
                </NyroButton>
                <NyroButton
                  onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
                  disabled={page >= totalPages - 1}
                  variant="icon"
                >
                  <ChevronRight className="h-4 w-4" />
                </NyroButton>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
