import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, Pencil, Plus, Route as RouteIcon, Trash2, X } from "lucide-react";

import { backend } from "@/lib/backend";
import type { CreateRoute, Provider, Route as RouteType, UpdateRoute } from "@/lib/types";
import { useLocale } from "@/lib/i18n";
import { ProviderIcon } from "@/components/ui/provider-icon";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Combobox } from "@/components/ui/combobox";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const PAGE_SIZE = 6;

type RouteForm = {
  name: string;
  ingress_protocol: "openai" | "anthropic" | "gemini";
  virtual_model: string;
  target_provider: string;
  target_model: string;
  access_control: boolean;
};

const emptyCreate: RouteForm = {
  name: "",
  ingress_protocol: "openai",
  virtual_model: "",
  target_provider: "",
  target_model: "",
  access_control: false,
};

function FieldLabel({ children }: { children: string }) {
  return <label className="ml-1 text-xs leading-none font-normal text-slate-900">{children}</label>;
}

function protocolLabel(value: RouteForm["ingress_protocol"]) {
  if (value === "anthropic") return "Anthropic";
  if (value === "gemini") return "Gemini";
  return "OpenAI";
}

export default function RoutesPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";
  const qc = useQueryClient();

  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [createForm, setCreateForm] = useState<RouteForm>(emptyCreate);
  const [editForm, setEditForm] = useState<(RouteForm & { id: string }) | null>(null);
  const [editError, setEditError] = useState<string | null>(null);

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
      setCreateForm(emptyCreate);
    },
  });
  const updateMut = useMutation({
    mutationFn: ({ id, input }: { id: string; input: UpdateRoute }) => backend("update_route", { id, input }),
    onSuccess: () => {
      setEditError(null);
      setEditingId(null);
      setEditForm(null);
      qc.invalidateQueries({ queryKey: ["routes"] });
    },
    onError: (err: Error) => setEditError(String(err)),
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => backend("delete_route", { id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["routes"] }),
  });

  const providerOptions = useMemo(
    () => providers.map((p) => ({ value: p.id, label: p.name, provider: p })),
    [providers],
  );
  const providerMap = useMemo(
    () => new Map(providers.map((p) => [p.id, p])),
    [providers],
  );

  function hasProviderModelsEndpoint(provider?: Provider) {
    return Boolean(provider?.models_endpoint?.trim());
  }
  function withCurrentModel(options: string[], current?: string) {
    if (!current || options.includes(current)) return options;
    return [current, ...options];
  }

  const createProvider = providerMap.get(createForm.target_provider);
  const editProvider = editForm ? providerMap.get(editForm.target_provider) : undefined;

  const { data: createTargetModels = [] } = useQuery<string[]>({
    queryKey: ["provider-models", createForm.target_provider],
    queryFn: () => backend("get_provider_models", { id: createForm.target_provider }),
    enabled: !!createForm.target_provider && hasProviderModelsEndpoint(createProvider),
    staleTime: 60_000,
  });
  const { data: editTargetModels = [] } = useQuery<string[]>({
    queryKey: ["provider-models", editForm?.target_provider],
    queryFn: () => backend("get_provider_models", { id: editForm?.target_provider }),
    enabled: !!editForm?.target_provider && hasProviderModelsEndpoint(editProvider),
    staleTime: 60_000,
  });

  const totalPages = Math.max(1, Math.ceil(routes.length / PAGE_SIZE));
  const pagedRoutes = routes.slice(page * PAGE_SIZE, page * PAGE_SIZE + PAGE_SIZE);

  useEffect(() => {
    if (page > totalPages - 1) setPage(0);
  }, [page, totalPages]);

  function startEdit(route: RouteType) {
    setEditingId(route.id);
    setEditError(null);
    setEditForm({
      id: route.id,
      name: route.name,
      ingress_protocol: route.ingress_protocol,
      virtual_model: route.virtual_model,
      target_provider: route.target_provider,
      target_model: route.target_model,
      access_control: route.access_control,
    });
  }

  function setCreateTargetModel(nextTargetModel: string) {
    setCreateForm((prev) => {
      const shouldInherit =
        !prev.virtual_model.trim() || prev.virtual_model.trim() === prev.target_model.trim();
      return {
        ...prev,
        target_model: nextTargetModel,
        virtual_model: shouldInherit ? nextTargetModel : prev.virtual_model,
      };
    });
  }

  function providerName(id: string) {
    return providers.find((p) => p.id === id)?.name ?? id.slice(0, 8);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">{isZh ? "路由" : "Routes"}</h1>
          <p className="mt-1 text-sm text-slate-500">
            {isZh ? "按接入协议 + 虚拟模型精确匹配" : "Exact match by ingress protocol and virtual model"}
          </p>
        </div>
        <Button
          onClick={() => {
            setEditingId(null);
            setEditForm(null);
            setShowForm((v) => !v);
          }}
          className="flex items-center gap-2"
        >
          <Plus className="h-4 w-4" />
          {isZh ? "新增路由" : "Add Route"}
        </Button>
      </div>

      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-slate-900">{isZh ? "新建路由" : "New Route"}</h2>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
              <Input
                value={createForm.name}
                onChange={(e) => setCreateForm((prev) => ({ ...prev, name: e.target.value }))}
                placeholder={isZh ? "输入路由名称" : "Enter route name"}
              />
            </div>
            <div className="space-y-2">
              <FieldLabel>{isZh ? "接入协议" : "Ingress Protocol"}</FieldLabel>
              <Select
                value={createForm.ingress_protocol}
                onValueChange={(value: "openai" | "anthropic" | "gemini") =>
                  setCreateForm((prev) => ({ ...prev, ingress_protocol: value }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai">OpenAI</SelectItem>
                  <SelectItem value="anthropic">Anthropic</SelectItem>
                  <SelectItem value="gemini">Gemini</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <FieldLabel>{isZh ? "目标提供商" : "Target Provider"}</FieldLabel>
              <Select
                value={createForm.target_provider || undefined}
                onValueChange={(value) => setCreateForm((prev) => ({ ...prev, target_provider: value }))}
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
            {hasProviderModelsEndpoint(createProvider) ? (
              <div className="space-y-2">
                <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                <Combobox
                  value={createForm.target_model}
                  options={withCurrentModel(createTargetModels, createForm.target_model).map((model) => ({
                    value: model,
                    label: model,
                  }))}
                  placeholder={isZh ? "选择目标模型" : "Select target model"}
                  searchPlaceholder={isZh ? "搜索模型..." : "Search model..."}
                  emptyText={isZh ? "暂无可用模型" : "No models available"}
                  onValueChange={setCreateTargetModel}
                />
              </div>
            ) : (
              <div className="space-y-2">
                <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                <Input
                  value={createForm.target_model}
                  onChange={(e) => setCreateTargetModel(e.target.value)}
                  placeholder={isZh ? "输入实际调用模型名" : "Enter target model"}
                />
              </div>
            )}
            <div className="space-y-2">
              <FieldLabel>{isZh ? "虚拟模型名" : "Virtual Model"}</FieldLabel>
              <Input
                value={createForm.virtual_model}
                onChange={(e) => setCreateForm((prev) => ({ ...prev, virtual_model: e.target.value }))}
                placeholder={isZh ? "客户端请求中的 model（精确匹配）" : "Client model (exact match)"}
              />
            </div>
            <div className="col-span-2 space-y-2">
              <FieldLabel>{isZh ? "访问控制（需 API Key）" : "Access Control (API Key required)"}</FieldLabel>
              <div className="pt-1">
                <Switch
                  id="create-route-access-control"
                  checked={createForm.access_control}
                  onCheckedChange={(checked) => setCreateForm((prev) => ({ ...prev, access_control: checked }))}
                />
              </div>
            </div>
          </div>
          <div className="flex gap-3">
            <Button
              onClick={() =>
                createMut.mutate({
                  name: createForm.name.trim(),
                  ingress_protocol: createForm.ingress_protocol,
                  virtual_model: createForm.virtual_model.trim(),
                  target_provider: createForm.target_provider,
                  target_model: createForm.target_model.trim(),
                  access_control: createForm.access_control,
                })
              }
              disabled={
                createMut.isPending ||
                !createForm.name.trim() ||
                !createForm.virtual_model.trim() ||
                !createForm.target_provider ||
                !createForm.target_model.trim()
              }
            >
              {createMut.isPending ? (isZh ? "创建中..." : "Creating...") : (isZh ? "创建" : "Create")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => {
                setShowForm(false);
                setCreateForm(emptyCreate);
              }}
            >
              {isZh ? "取消" : "Cancel"}
            </Button>
          </div>
        </div>
      )}

      {isLoading ? (
        <div className="py-12 text-center text-sm text-slate-500">{isZh ? "加载中..." : "Loading..."}</div>
      ) : routes.length === 0 ? (
        <div className="glass rounded-2xl p-12 text-center">
          <RouteIcon className="mx-auto h-10 w-10 text-slate-400" />
          <p className="mt-3 text-sm text-slate-500">{isZh ? "还没有配置路由" : "No routes configured"}</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {pagedRoutes.map((route) => {
            const isEditing = editingId === route.id && editForm;
            const targetProvider = providerMap.get(route.target_provider);

            if (isEditing && editForm) {
              return (
                <div key={route.id} className="glass rounded-2xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-900">{isZh ? "编辑路由" : "Edit Route"}</h3>
                    <button
                      onClick={() => {
                        setEditingId(null);
                        setEditForm(null);
                        setEditError(null);
                      }}
                      className="cursor-pointer p-1 text-slate-400 hover:text-slate-600"
                    >
                      <X className="h-4 w-4" />
                    </button>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
                      <Input
                        value={editForm.name}
                        onChange={(e) => setEditForm((prev) => (prev ? { ...prev, name: e.target.value } : prev))}
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "接入协议" : "Ingress Protocol"}</FieldLabel>
                      <Select
                        value={editForm.ingress_protocol}
                        onValueChange={(value: "openai" | "anthropic" | "gemini") =>
                          setEditForm((prev) => (prev ? { ...prev, ingress_protocol: value } : prev))
                        }
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="openai">OpenAI</SelectItem>
                          <SelectItem value="anthropic">Anthropic</SelectItem>
                          <SelectItem value="gemini">Gemini</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "目标提供商" : "Target Provider"}</FieldLabel>
                      <Select
                        value={editForm.target_provider}
                        onValueChange={(value) =>
                          setEditForm((prev) => (prev ? { ...prev, target_provider: value } : prev))
                        }
                      >
                        <SelectTrigger>
                          <SelectValue />
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
                    {hasProviderModelsEndpoint(editProvider) ? (
                      <div className="space-y-2">
                        <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                        <Combobox
                          value={editForm.target_model}
                          options={withCurrentModel(editTargetModels, editForm.target_model).map((model) => ({
                            value: model,
                            label: model,
                          }))}
                          placeholder={isZh ? "选择目标模型" : "Select target model"}
                          searchPlaceholder={isZh ? "搜索模型..." : "Search model..."}
                          emptyText={isZh ? "暂无可用模型" : "No models available"}
                          onValueChange={(value) =>
                            setEditForm((prev) => (prev ? { ...prev, target_model: value } : prev))
                          }
                        />
                      </div>
                    ) : (
                      <div className="space-y-2">
                        <FieldLabel>{isZh ? "目标模型" : "Target Model"}</FieldLabel>
                        <Input
                          value={editForm.target_model}
                          onChange={(e) =>
                            setEditForm((prev) => (prev ? { ...prev, target_model: e.target.value } : prev))
                          }
                        />
                      </div>
                    )}
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "虚拟模型名" : "Virtual Model"}</FieldLabel>
                      <Input
                        value={editForm.virtual_model}
                        onChange={(e) =>
                          setEditForm((prev) => (prev ? { ...prev, virtual_model: e.target.value } : prev))
                        }
                      />
                    </div>
                    <div className="col-span-2 space-y-2">
                      <FieldLabel>{isZh ? "访问控制（需 API Key）" : "Access Control (API Key required)"}</FieldLabel>
                      <div className="pt-1">
                        <Switch
                          checked={editForm.access_control}
                          onCheckedChange={(checked) =>
                            setEditForm((prev) => (prev ? { ...prev, access_control: checked } : prev))
                          }
                        />
                      </div>
                    </div>
                  </div>
                  <div className="flex gap-3">
                    <Button
                      onClick={() =>
                        updateMut.mutate({
                          id: editForm.id,
                          input: {
                            name: editForm.name.trim(),
                            ingress_protocol: editForm.ingress_protocol,
                            virtual_model: editForm.virtual_model.trim(),
                            target_provider: editForm.target_provider,
                            target_model: editForm.target_model.trim(),
                            access_control: editForm.access_control,
                          },
                        })
                      }
                      disabled={updateMut.isPending}
                    >
                      {updateMut.isPending ? (isZh ? "保存中..." : "Saving...") : (isZh ? "保存" : "Save")}
                    </Button>
                    <Button
                      variant="secondary"
                      onClick={() => {
                        setEditingId(null);
                        setEditForm(null);
                        setEditError(null);
                      }}
                    >
                      {isZh ? "取消" : "Cancel"}
                    </Button>
                  </div>
                  {editError && <p className="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600">{editError}</p>}
                </div>
              );
            }

            return (
              <div key={route.id} className="glass flex items-center justify-between rounded-2xl p-5">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-900">{route.name}</span>
                    <code className="rounded bg-slate-100 px-2 py-0.5 text-[11px] text-slate-600">
                      {protocolLabel(route.ingress_protocol)} / {route.virtual_model}
                    </code>
                    {route.access_control && (
                      <Badge variant="warning">
                        {isZh ? "鉴权" : "Auth"}
                      </Badge>
                    )}
                    {!route.is_active && (
                      <Badge variant="danger">
                        {isZh ? "停用" : "Inactive"}
                      </Badge>
                    )}
                  </div>
                  <div className="mt-1.5 flex items-center gap-2 text-xs">
                    <span className="route-flow-pill inline-flex items-center gap-1.5 rounded-full px-2.5 py-1">
                      <ProviderIcon
                        name={targetProvider?.name}
                        protocol={targetProvider?.protocol}
                        baseUrl={targetProvider?.base_url}
                        size={14}
                        className="rounded-sm border-0 bg-transparent"
                      />
                      <span className="font-medium text-slate-600">{providerName(route.target_provider)}</span>
                      <span className="text-slate-400">→</span>
                      <span className="font-medium text-slate-700">{route.target_model}</span>
                    </span>
                  </div>
                </div>
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => startEdit(route)}
                    className="cursor-pointer rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-500"
                  >
                    <Pencil className="h-4 w-4" />
                  </button>
                  <button
                    onClick={() => deleteMut.mutate(route.id)}
                    className="cursor-pointer rounded-lg p-2 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
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
                <Button
                  onClick={() => setPage(Math.max(0, page - 1))}
                  disabled={page === 0}
                  variant="outline"
                  size="icon"
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                <Button
                  onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
                  disabled={page >= totalPages - 1}
                  variant="outline"
                  size="icon"
                >
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
