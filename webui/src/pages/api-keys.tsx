import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, Copy, KeyRound, Pencil, Plus, Trash2, X } from "lucide-react";

import { backend } from "@/lib/backend";
import type { ApiKey, CreateApiKey, Route as RouteType, UpdateApiKey } from "@/lib/types";
import { useLocale } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { MultiSelect } from "@/components/ui/multi-select";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const PAGE_SIZE = 6;

type ExpirePreset = "never" | "1d" | "7d" | "30d" | "90d" | "180d" | "1y";

const expirePresetOptions: { value: ExpirePreset; zh: string; en: string }[] = [
  { value: "never", zh: "永不过期", en: "Never" },
  { value: "1d", zh: "1 天", en: "1 day" },
  { value: "7d", zh: "7 天", en: "7 days" },
  { value: "30d", zh: "30 天", en: "30 days" },
  { value: "90d", zh: "90 天", en: "90 days" },
  { value: "180d", zh: "180 天", en: "180 days" },
  { value: "1y", zh: "1 年", en: "1 year" },
];

function FieldLabel({ children }: { children: string }) {
  return <label className="ml-1 text-xs leading-none font-normal text-slate-900">{children}</label>;
}

function maskApiKey(key: string) {
  if (key.length <= 12) return key;
  return `${key.slice(0, 10)}••••••`;
}

function toSqlDatetime(value?: string | null) {
  if (!value) return undefined;
  return value.replace("T", " ").slice(0, 19);
}

function fromSqlDatetime(value?: string | null) {
  if (!value) return "";
  return value.replace(" ", "T").slice(0, 16);
}

function resolveExpiresAt(preset: ExpirePreset) {
  if (preset === "never") return undefined;
  const now = Date.now();
  const day = 24 * 60 * 60 * 1000;
  const map: Record<Exclude<ExpirePreset, "never">, number> = {
    "1d": 1,
    "7d": 7,
    "30d": 30,
    "90d": 90,
    "180d": 180,
    "1y": 365,
  };
  const date = new Date(now + map[preset] * day);
  return date.toISOString().slice(0, 19).replace("T", " ");
}

type CreateForm = {
  name: string;
  rpm: string;
  tpm: string;
  tpd: string;
  expiresPreset: ExpirePreset;
  route_ids: string[];
};

type EditForm = {
  id: string;
  name: string;
  rpm: string;
  tpm: string;
  tpd: string;
  expires_at: string;
  route_ids: string[];
};

const emptyCreate: CreateForm = {
  name: "",
  rpm: "",
  tpm: "",
  tpd: "",
  expiresPreset: "30d",
  route_ids: [],
};

export default function ApiKeysPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";
  const qc = useQueryClient();

  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [createForm, setCreateForm] = useState<CreateForm>(emptyCreate);
  const [editForm, setEditForm] = useState<EditForm | null>(null);
  const [page, setPage] = useState(0);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [showCreatedDialog, setShowCreatedDialog] = useState(false);

  const { data: apiKeys = [], isLoading } = useQuery<ApiKey[]>({
    queryKey: ["api-keys"],
    queryFn: () => backend("list_api_keys"),
  });
  const { data: routes = [] } = useQuery<RouteType[]>({
    queryKey: ["routes"],
    queryFn: () => backend("list_routes"),
  });

  const createMut = useMutation({
    mutationFn: (input: CreateApiKey) => backend<ApiKey>("create_api_key", { input }),
    onSuccess: (created) => {
      qc.invalidateQueries({ queryKey: ["api-keys"] });
      setShowForm(false);
      setCreateForm(emptyCreate);
      setCreatedKey(created.key);
      setShowCreatedDialog(true);
    },
  });
  const updateMut = useMutation({
    mutationFn: ({ id, input }: { id: string; input: UpdateApiKey }) => backend("update_api_key", { id, input }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["api-keys"] });
      setEditingId(null);
      setEditForm(null);
    },
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => backend("delete_api_key", { id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["api-keys"] }),
  });

  const totalPages = Math.max(1, Math.ceil(apiKeys.length / PAGE_SIZE));
  const pagedApiKeys = apiKeys.slice(page * PAGE_SIZE, page * PAGE_SIZE + PAGE_SIZE);

  useEffect(() => {
    if (page > totalPages - 1) setPage(0);
  }, [page, totalPages]);

  const routeMap = useMemo(() => new Map(routes.map((route) => [route.id, route])), [routes]);
  const routeOptions = useMemo(
    () =>
      routes.map((route) => ({
        value: route.id,
        label: `${route.name} (${route.ingress_protocol} / ${route.virtual_model})`,
      })),
    [routes],
  );

  function startEdit(item: ApiKey) {
    setEditingId(item.id);
    setEditForm({
      id: item.id,
      name: item.name,
      rpm: item.rpm ? String(item.rpm) : "",
      tpm: item.tpm ? String(item.tpm) : "",
      tpd: item.tpd ? String(item.tpd) : "",
      expires_at: fromSqlDatetime(item.expires_at),
      route_ids: item.route_ids ?? [],
    });
  }

  async function copyKey(item: ApiKey) {
    await navigator.clipboard.writeText(item.key);
    setCopiedId(item.id);
    setTimeout(() => setCopiedId(null), 1500);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">{isZh ? "API Key" : "API Keys"}</h1>
          <p className="mt-1 text-sm text-slate-500">
            {isZh ? "管理鉴权 Key、配额与路由绑定" : "Manage authentication keys, quotas, and route bindings"}
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
          {isZh ? "新增 Key" : "Add Key"}
        </Button>
      </div>

      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-slate-900">{isZh ? "创建 API Key" : "Create API Key"}</h2>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
              <Input
                value={createForm.name}
                onChange={(e) => setCreateForm((prev) => ({ ...prev, name: e.target.value }))}
                placeholder={isZh ? "例如 Frontend App" : "e.g. Frontend App"}
              />
            </div>
            <div className="space-y-2">
              <FieldLabel>{isZh ? "过期时间" : "Expiration"}</FieldLabel>
              <Select
                value={createForm.expiresPreset}
                onValueChange={(value: ExpirePreset) =>
                  setCreateForm((prev) => ({ ...prev, expiresPreset: value }))
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {expirePresetOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {isZh ? option.zh : option.en}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <FieldLabel>RPM</FieldLabel>
              <Input
                type="number"
                min={0}
                value={createForm.rpm}
                onChange={(e) => setCreateForm((prev) => ({ ...prev, rpm: e.target.value }))}
                placeholder={isZh ? "留空=不限" : "Empty = unlimited"}
              />
            </div>
            <div className="space-y-2">
              <FieldLabel>TPM</FieldLabel>
              <Input
                type="number"
                min={0}
                value={createForm.tpm}
                onChange={(e) => setCreateForm((prev) => ({ ...prev, tpm: e.target.value }))}
                placeholder={isZh ? "留空=不限" : "Empty = unlimited"}
              />
            </div>
            <div className="col-span-2 space-y-2">
              <FieldLabel>TPD</FieldLabel>
              <Input
                type="number"
                min={0}
                value={createForm.tpd}
                onChange={(e) => setCreateForm((prev) => ({ ...prev, tpd: e.target.value }))}
                placeholder={isZh ? "留空=不限" : "Empty = unlimited"}
              />
            </div>
            <div className="col-span-2 space-y-2">
              <FieldLabel>{isZh ? "绑定路由（不勾选=全局生效）" : "Bind Routes (none = global access)"}</FieldLabel>
              <MultiSelect
                options={routeOptions}
                values={createForm.route_ids}
                placeholder={isZh ? "选择绑定路由（为空=全局）" : "Select routes (empty = global)"}
                searchPlaceholder={isZh ? "搜索路由..." : "Search routes..."}
                emptyText={isZh ? "无匹配路由" : "No matching routes"}
                onChange={(next) => setCreateForm((prev) => ({ ...prev, route_ids: next }))}
              />
            </div>
          </div>
          <div className="flex gap-3">
            <Button
              onClick={() =>
                createMut.mutate({
                  name: createForm.name.trim(),
                  rpm: createForm.rpm ? Number.parseInt(createForm.rpm, 10) : undefined,
                  tpm: createForm.tpm ? Number.parseInt(createForm.tpm, 10) : undefined,
                  tpd: createForm.tpd ? Number.parseInt(createForm.tpd, 10) : undefined,
                  expires_at: resolveExpiresAt(createForm.expiresPreset),
                  route_ids: createForm.route_ids,
                })
              }
              disabled={createMut.isPending || !createForm.name.trim()}
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
      ) : apiKeys.length === 0 ? (
        <div className="glass rounded-2xl p-12 text-center">
          <KeyRound className="mx-auto h-10 w-10 text-slate-400" />
          <p className="mt-3 text-sm text-slate-500">{isZh ? "还没有 API Key" : "No API keys yet"}</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {pagedApiKeys.map((item) => {
            const isEditing = editingId === item.id && editForm;
            if (isEditing && editForm) {
              return (
                <div key={item.id} className="glass rounded-2xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-900">{isZh ? "编辑 API Key" : "Edit API Key"}</h3>
                    <button
                      onClick={() => {
                        setEditingId(null);
                        setEditForm(null);
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
                      <FieldLabel>RPM</FieldLabel>
                      <Input
                        type="number"
                        min={0}
                        value={editForm.rpm}
                        onChange={(e) => setEditForm((prev) => (prev ? { ...prev, rpm: e.target.value } : prev))}
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>TPM</FieldLabel>
                      <Input
                        type="number"
                        min={0}
                        value={editForm.tpm}
                        onChange={(e) => setEditForm((prev) => (prev ? { ...prev, tpm: e.target.value } : prev))}
                      />
                    </div>
                    <div className="col-span-2 space-y-2">
                      <FieldLabel>TPD</FieldLabel>
                      <Input
                        type="number"
                        min={0}
                        value={editForm.tpd}
                        onChange={(e) => setEditForm((prev) => (prev ? { ...prev, tpd: e.target.value } : prev))}
                      />
                    </div>
                    <div className="col-span-2 space-y-2">
                      <FieldLabel>{isZh ? "过期时间（留空=永不过期）" : "Expires At (empty = never)"}</FieldLabel>
                      <Input
                        type="datetime-local"
                        value={editForm.expires_at}
                        onChange={(e) =>
                          setEditForm((prev) => (prev ? { ...prev, expires_at: e.target.value } : prev))
                        }
                      />
                    </div>
                    <div className="col-span-2 space-y-2">
                      <FieldLabel>{isZh ? "绑定路由（不勾选=全局生效）" : "Bind Routes (none = global access)"}</FieldLabel>
                      <MultiSelect
                        options={routeOptions}
                        values={editForm.route_ids}
                        placeholder={isZh ? "选择绑定路由（为空=全局）" : "Select routes (empty = global)"}
                        searchPlaceholder={isZh ? "搜索路由..." : "Search routes..."}
                        emptyText={isZh ? "无匹配路由" : "No matching routes"}
                        onChange={(next) =>
                          setEditForm((prev) => (prev ? { ...prev, route_ids: next } : prev))
                        }
                      />
                    </div>
                  </div>
                  <div className="flex gap-3">
                    <Button
                      onClick={() =>
                        updateMut.mutate({
                          id: editForm.id,
                          input: {
                            name: editForm.name.trim(),
                            rpm: editForm.rpm ? Number.parseInt(editForm.rpm, 10) : undefined,
                            tpm: editForm.tpm ? Number.parseInt(editForm.tpm, 10) : undefined,
                            tpd: editForm.tpd ? Number.parseInt(editForm.tpd, 10) : undefined,
                            expires_at: toSqlDatetime(editForm.expires_at),
                            route_ids: editForm.route_ids,
                          },
                        })
                      }
                      disabled={updateMut.isPending}
                    >
                      {updateMut.isPending ? (isZh ? "保存中..." : "Saving...") : (isZh ? "保存" : "Save")}
                    </Button>
                    <Button variant="secondary" onClick={() => setEditingId(null)}>
                      {isZh ? "取消" : "Cancel"}
                    </Button>
                  </div>
                </div>
              );
            }

            return (
              <div key={item.id} className="glass flex items-center justify-between rounded-2xl p-5">
                <div className="space-y-1">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-900">{item.name}</span>
                    <Badge variant={item.status === "active" ? "success" : "danger"}>
                      {item.status}
                    </Badge>
                  </div>
                  <p className="text-xs text-slate-500">
                    {maskApiKey(item.key)}
                    {" · "}
                    RPM {item.rpm ?? "∞"} / TPM {item.tpm ?? "∞"} / TPD {item.tpd ?? "∞"}
                  </p>
                  <p className="text-xs text-slate-500">
                    {isZh ? "绑定路由" : "Bound routes"}:{" "}
                    {item.route_ids.length
                      ? item.route_ids
                          .map((id) => routeMap.get(id)?.name ?? id.slice(0, 8))
                          .join(", ")
                      : (isZh ? "全局" : "Global")}
                  </p>
                </div>
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => copyKey(item)}
                    className="cursor-pointer rounded-lg p-2 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700"
                  >
                    <Copy className="h-4 w-4" />
                  </button>
                  <button
                    onClick={() => startEdit(item)}
                    className="cursor-pointer rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-500"
                  >
                    <Pencil className="h-4 w-4" />
                  </button>
                  <button
                    onClick={() => deleteMut.mutate(item.id)}
                    className="cursor-pointer rounded-lg p-2 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
                {copiedId === item.id && (
                  <span className="ml-2 text-xs text-green-600">{isZh ? "已复制" : "Copied"}</span>
                )}
              </div>
            );
          })}

          {apiKeys.length > PAGE_SIZE && (
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

      <Dialog open={showCreatedDialog} onOpenChange={setShowCreatedDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{isZh ? "API Key 创建成功" : "API Key Created"}</DialogTitle>
            <DialogDescription>
              {isZh
                ? "请立即复制并保存该 Key，后续仅显示脱敏值。"
                : "Copy and save this key now. It will be masked in later views."}
            </DialogDescription>
          </DialogHeader>
          <div className="rounded-xl bg-slate-900 px-4 py-3 text-sm text-green-400">
            {createdKey ?? "-"}
          </div>
          <DialogFooter>
            <Button
              variant="secondary"
              onClick={() => setShowCreatedDialog(false)}
            >
              {isZh ? "关闭" : "Close"}
            </Button>
            <Button
              onClick={async () => {
                if (!createdKey) return;
                await navigator.clipboard.writeText(createdKey);
              }}
            >
              {isZh ? "复制 Key" : "Copy Key"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
