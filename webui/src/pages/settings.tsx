import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, useRef } from "react";
import { backend, IS_TAURI } from "@/lib/backend";
import { localizeBackendErrorMessage } from "@/lib/backend-error";
import type {
  CacheSettings,
  ExportData,
  GatewayStatus,
  ImportResult,
  Provider,
  Route as RouteType,
} from "@/lib/types";
import { useLocale } from "@/lib/i18n";
import {
  Download,
  Upload,
  Save,
  Loader2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

type CacheToggleDialogState =
  | { kind: "exact"; next: CacheSettings }
  | { kind: "semantic"; next: CacheSettings }
  | { kind: "semantic_missing_route" };
type ProxyToggleDialogState =
  | { kind: "confirm"; nextEnabled: boolean }
  | { kind: "missing_url" };

export default function SettingsPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";
  const appVersion = import.meta.env.VITE_APP_VERSION;

  const qc = useQueryClient();
  const fileRef = useRef<HTMLInputElement>(null);
  const [errorDialog, setErrorDialog] = useState<{ title: string; description?: string } | null>(null);
  const [cacheToggleDialog, setCacheToggleDialog] = useState<CacheToggleDialogState | null>(null);
  const [proxyToggleDialog, setProxyToggleDialog] = useState<ProxyToggleDialogState | null>(null);

  const { data: status } = useQuery<GatewayStatus>({
    queryKey: ["gateway-status"],
    queryFn: () => backend("get_gateway_status"),
  });

  const { data: retentionDays } = useQuery<string | null>({
    queryKey: ["setting", "log_retention_days"],
    queryFn: () => backend("get_setting", { key: "log_retention_days" }),
  });
  const { data: proxyEnabledSetting } = useQuery<string | null>({
    queryKey: ["setting", "proxy_enabled"],
    queryFn: () => backend("get_setting", { key: "proxy_enabled" }),
  });
  const { data: proxyUrlSetting } = useQuery<string | null>({
    queryKey: ["setting", "proxy_url"],
    queryFn: () => backend("get_setting", { key: "proxy_url" }),
  });
  const { data: proxyBypassSetting } = useQuery<string | null>({
    queryKey: ["setting", "proxy_bypass"],
    queryFn: () => backend("get_setting", { key: "proxy_bypass" }),
  });
  const { data: cacheSettings } = useQuery<CacheSettings>({
    queryKey: ["cache-settings"],
    queryFn: () => backend("get_cache_settings"),
  });
  const { data: routes = [] } = useQuery<RouteType[]>({
    queryKey: ["routes"],
    queryFn: () => backend("list_routes"),
  });

  const [retentionInput, setRetentionInput] = useState<string>("");
  const retentionValue = retentionInput || retentionDays || "30";
  const [proxyEnabled, setProxyEnabled] = useState(false);
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyBypass, setProxyBypass] = useState("");
  const [cacheForm, setCacheForm] = useState<CacheSettings>({
    exact: {
      enabled: false,
      default_ttl: 3600,
      max_entries: 1000,
    },
    semantic: {
      enabled: false,
      embedding_route: "",
      similarity_threshold: 0.92,
      vector_dimensions: 1536,
      default_ttl: 600,
      max_entries: 500,
    },
  });
  const embeddingRoutes = routes.filter((route) => route.route_type === "embedding");
  const selectedEmbeddingRouteExists = embeddingRoutes.some(
    (route) => route.virtual_model === cacheForm.semantic.embedding_route,
  );

  useEffect(() => {
    const normalized = (proxyEnabledSetting ?? "").trim().toLowerCase();
    setProxyEnabled(["1", "true", "yes", "on"].includes(normalized));
    setProxyUrl(proxyUrlSetting ?? "");
    setProxyBypass(proxyBypassSetting ?? "");
  }, [proxyEnabledSetting, proxyUrlSetting, proxyBypassSetting]);

  useEffect(() => {
    if (cacheSettings) {
      setCacheForm(cacheSettings);
    }
  }, [cacheSettings]);

  function formatErrorMessage(error: unknown) {
    return localizeBackendErrorMessage(error, isZh);
  }

  function showErrorDialog(titleZh: string, titleEn: string, error: unknown) {
    setErrorDialog({
      title: isZh ? titleZh : titleEn,
      description: formatErrorMessage(error),
    });
  }

  const saveSetting = useMutation({
    mutationFn: (value: string) =>
      backend("set_setting", { key: "log_retention_days", value }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["setting", "log_retention_days"] }),
    onError: (error: unknown) => {
      showErrorDialog("保存设置失败", "Failed to save settings", error);
    },
  });
  const saveProxyToggle = useMutation({
    mutationFn: async (input: { enabled: boolean; url: string; bypass: string }) => {
      await Promise.all([
        backend("set_setting", { key: "proxy_enabled", value: input.enabled ? "true" : "false" }),
        backend("set_setting", { key: "proxy_url", value: input.url }),
        backend("set_setting", { key: "proxy_bypass", value: input.bypass }),
      ]);

      // If global proxy is turned off, force all providers to disable provider-level proxy usage.
      if (!input.enabled) {
        const providers = await backend<Provider[]>("get_providers");
        const providersUsingProxy = providers.filter((provider) => provider.use_proxy);
        await Promise.all(
          providersUsingProxy.map((provider) =>
            backend("update_provider", {
              id: provider.id,
              input: { use_proxy: false },
            }),
          ),
        );
      }
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["setting", "proxy_enabled"] });
      qc.invalidateQueries({ queryKey: ["setting", "proxy_url"] });
      qc.invalidateQueries({ queryKey: ["setting", "proxy_bypass"] });
      qc.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error: unknown) => {
      setProxyToggleDialog(null);
      showErrorDialog("保存代理设置失败", "Failed to save proxy settings", error);
    },
  });
  const saveExactCacheToggle = useMutation({
    mutationFn: (next: CacheSettings) => backend("update_cache_settings", { input: next }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["cache-settings"] });
    },
    onError: (error: unknown) => {
      setCacheToggleDialog(null);
      showErrorDialog("保存精确匹配缓存设置失败", "Failed to save exact cache settings", error);
    },
  });
  const saveSemanticCacheToggle = useMutation({
    mutationFn: (next: CacheSettings) => backend("update_cache_settings", { input: next }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["cache-settings"] });
    },
    onError: (error: unknown) => {
      setCacheToggleDialog(null);
      showErrorDialog("保存语义相似缓存设置失败", "Failed to save semantic cache settings", error);
    },
  });
  const detectEmbeddingDimensionsMut = useMutation({
    mutationFn: (embeddingRoute: string) =>
      backend<number>("detect_embedding_dimensions", { embeddingRoute }),
    onSuccess: (dimensions) => {
      setCacheForm((prev) => ({
        ...prev,
        semantic: { ...prev.semantic, vector_dimensions: Math.max(1, Number(dimensions || 1)) },
      }));
    },
    onError: (error: unknown) => {
      showErrorDialog("自动探测向量维度失败", "Failed to detect embedding dimensions", error);
    },
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
    onError: (error: unknown) => {
      showErrorDialog("导出配置失败", "Failed to export config", error);
    },
  });

  const importMut = useMutation({
    mutationFn: (data: ExportData) =>
      backend<ImportResult>("import_config", { data }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
      qc.invalidateQueries({ queryKey: ["routes"] });
    },
    onError: (error: unknown) => {
      showErrorDialog("导入配置失败", "Failed to import config", error);
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
        setErrorDialog({
          title: isZh ? "导入配置失败" : "Failed to import config",
          description: isZh ? "无效的 JSON 文件" : "Invalid JSON file",
        });
      }
    };
    reader.readAsText(file);
    e.target.value = "";
  }

  function handleExactEnabledToggle(checked: boolean) {
    const next: CacheSettings = {
      ...cacheForm,
      exact: { ...cacheForm.exact, enabled: checked },
    };
    if (checked) {
      setCacheToggleDialog({ kind: "exact", next });
      return;
    }
    saveExactCacheToggle.mutate(next, {
      onSuccess: () => {
        setCacheForm(next);
      },
    });
  }

  function handleSemanticEnabledToggle(checked: boolean) {
    const next: CacheSettings = {
      ...cacheForm,
      semantic: { ...cacheForm.semantic, enabled: checked },
    };
    if (checked) {
      const hasSelectedEmbeddingRoute =
        cacheForm.semantic.embedding_route.trim().length > 0 && selectedEmbeddingRouteExists;
      if (!hasSelectedEmbeddingRoute) {
        setCacheToggleDialog({ kind: "semantic_missing_route" });
        return;
      }
      setCacheToggleDialog({ kind: "semantic", next });
      return;
    }
    saveSemanticCacheToggle.mutate(next, {
      onSuccess: () => {
        setCacheForm(next);
      },
    });
  }

  function handleProxyEnabledToggle(checked: boolean) {
    if (checked && !proxyUrl.trim()) {
      setProxyToggleDialog({ kind: "missing_url" });
      return;
    }
    setProxyToggleDialog({ kind: "confirm", nextEnabled: checked });
  }

  function handleConfirmCacheToggle() {
    if (!cacheToggleDialog) return;
    if (cacheToggleDialog.kind === "semantic_missing_route") {
      setCacheToggleDialog(null);
      return;
    }

    if (cacheToggleDialog.kind === "exact") {
      const next = cacheToggleDialog.next;
      saveExactCacheToggle.mutate(next, {
        onSuccess: () => {
          setCacheForm(next);
          setCacheToggleDialog(null);
        },
      });
      return;
    }

    const next = cacheToggleDialog.next;
    saveSemanticCacheToggle.mutate(next, {
      onSuccess: () => {
        setCacheForm(next);
        setCacheToggleDialog(null);
      },
    });
  }

  function handleEmbeddingRouteChange(value: string) {
    setCacheForm((prev) => ({
      ...prev,
      semantic: {
        ...prev.semantic,
        embedding_route: value,
      },
    }));
    const route = value.trim();
    if (route) {
      detectEmbeddingDimensionsMut.mutate(route);
    }
  }

  function handleConfirmProxyToggle() {
    if (!proxyToggleDialog) return;
    if (proxyToggleDialog.kind === "missing_url") {
      setProxyToggleDialog(null);
      return;
    }
    const nextEnabled = proxyToggleDialog.nextEnabled;
    saveProxyToggle.mutate(
      {
        enabled: nextEnabled,
        url: proxyUrl.trim(),
        bypass: proxyBypass.trim(),
      },
      {
        onSuccess: () => {
          setProxyEnabled(nextEnabled);
          setProxyToggleDialog(null);
        },
      },
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-slate-900">{isZh ? "设置" : "Settings"}</h1>
        <p className="mt-1 text-sm text-slate-500">
          {isZh ? "网关配置" : "Gateway configuration"}
        </p>
      </div>

      {/* Gateway Status */}
      <div className="glass rounded-2xl p-6 space-y-4">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "网关状态" : "Gateway Status"}</h2>
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">{isZh ? "状态" : "Status"}</p>
            <p className="mt-1 font-semibold text-green-600">{status?.status ?? "–"}</p>
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">{isZh ? "代理端口" : "Proxy Port"}</p>
            <p className="mt-1 font-semibold text-slate-900">{status?.proxy_port ?? "–"}</p>
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">{isZh ? "模式" : "Mode"}</p>
            <p className="mt-1 font-semibold text-slate-900">{IS_TAURI ? (isZh ? "桌面版" : "Desktop") : "Server"}</p>
          </div>
          <div className="rounded-xl bg-slate-50 p-4">
            <p className="text-xs text-slate-500">{isZh ? "版本" : "Version"}</p>
            <p className="mt-1 font-semibold text-slate-900">{appVersion}</p>
          </div>
        </div>
      </div>

      <div className="glass rounded-2xl p-6 space-y-5">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "缓存配置" : "Cache Configuration"}</h2>
        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div className="flex items-center justify-between">
              <p className="text-sm font-medium text-slate-700">{isZh ? "精确匹配缓存" : "Exact Cache"}</p>
              <Switch
                checked={cacheForm.exact.enabled}
                disabled={saveExactCacheToggle.isPending}
                onCheckedChange={handleExactEnabledToggle}
              />
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">TTL (s)</label>
                <Input
                  type="number"
                  min={1}
                  value={cacheForm.exact.default_ttl}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      exact: { ...prev.exact, default_ttl: Math.max(1, Number(e.target.value || 1)) },
                    }))
                  }
                />
              </div>
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "最大条目" : "Max Entries"}</label>
                <Input
                  type="number"
                  min={1}
                  value={cacheForm.exact.max_entries}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      exact: { ...prev.exact, max_entries: Math.max(1, Number(e.target.value || 1)) },
                    }))
                  }
                />
              </div>
            </div>
          </div>

          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div className="flex items-center justify-between">
              <p className="text-sm font-medium text-slate-700">{isZh ? "语义相似缓存" : "Semantic Cache"}</p>
              <Switch
                checked={cacheForm.semantic.enabled}
                disabled={saveSemanticCacheToggle.isPending}
                onCheckedChange={handleSemanticEnabledToggle}
              />
            </div>
            <div className="space-y-3">
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">
                  {isZh ? "Embedding 路由（必选）" : "Embedding Route (Required)"}
                </label>
                <Select
                  value={cacheForm.semantic.embedding_route || undefined}
                  onValueChange={handleEmbeddingRouteChange}
                >
                  <SelectTrigger>
                    <SelectValue
                      placeholder={isZh ? "请选择" : "Please select"}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {embeddingRoutes.map((route) => (
                      <SelectItem key={route.id} value={route.virtual_model}>
                        {route.name} ({route.virtual_model})
                      </SelectItem>
                    ))}
                    {!selectedEmbeddingRouteExists && cacheForm.semantic.embedding_route.trim() && (
                      <SelectItem value={cacheForm.semantic.embedding_route}>
                        {isZh
                          ? `当前值（未匹配到 embedding 路由）: ${cacheForm.semantic.embedding_route}`
                          : `Current value (not an embedding route): ${cacheForm.semantic.embedding_route}`}
                      </SelectItem>
                    )}
                    {embeddingRoutes.length === 0 && (
                      <SelectItem value="__empty__" disabled>
                        {isZh ? "暂无向量路由" : "No embedding routes"}
                      </SelectItem>
                    )}
                  </SelectContent>
                </Select>
                {detectEmbeddingDimensionsMut.isPending && (
                  <p className="ml-1 text-[11px] text-slate-500">
                    {isZh ? "正在自动探测向量维度..." : "Detecting embedding dimensions..."}
                  </p>
                )}
              </div>
              <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "阈值" : "Threshold"}</label>
                <Input
                  type="number"
                  step="0.01"
                  min={0}
                  max={1}
                  value={cacheForm.semantic.similarity_threshold}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      semantic: { ...prev.semantic, similarity_threshold: Number(e.target.value || 0) },
                    }))
                  }
                />
              </div>
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "向量维度" : "Dimensions"}</label>
                <Input
                  type="number"
                  min={1}
                  value={cacheForm.semantic.vector_dimensions}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      semantic: { ...prev.semantic, vector_dimensions: Math.max(1, Number(e.target.value || 1)) },
                    }))
                  }
                />
              </div>
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">TTL (s)</label>
                <Input
                  type="number"
                  min={1}
                  value={cacheForm.semantic.default_ttl}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      semantic: { ...prev.semantic, default_ttl: Math.max(1, Number(e.target.value || 1)) },
                    }))
                  }
                />
              </div>
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "最大条目" : "Max Entries"}</label>
                <Input
                  type="number"
                  min={1}
                  value={cacheForm.semantic.max_entries}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      semantic: { ...prev.semantic, max_entries: Math.max(1, Number(e.target.value || 1)) },
                    }))
                  }
                />
              </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="glass rounded-2xl p-6 space-y-5">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "代理配置" : "Proxy Configuration"}</h2>
        <div className="rounded-xl bg-slate-50 p-4 space-y-3">
          <div>
            <p className="text-sm font-medium text-slate-700">{isZh ? "本地代理" : "Local Proxy"}</p>
            <p className="text-xs text-slate-500">
              {isZh ? "Provider 开启代理后会使用这里的代理地址发送请求" : "Providers with proxy enabled will route requests through this proxy"}
            </p>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2.5">
            <span className="text-sm text-slate-700">{isZh ? "启用代理" : "Enable proxy"}</span>
            <Switch
              checked={proxyEnabled}
              disabled={saveProxyToggle.isPending}
              onCheckedChange={handleProxyEnabledToggle}
            />
          </div>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div className="space-y-1.5">
              <label className="ml-1 text-xs text-slate-700">{isZh ? "代理 URL" : "Proxy URL"}</label>
              <Input
                placeholder="http://127.0.0.1:7890"
                value={proxyUrl}
                onChange={(e) => setProxyUrl(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <label className="ml-1 text-xs text-slate-700">{isZh ? "绕过地址（可选）" : "Bypass hosts (optional)"}</label>
              <Input
                placeholder={isZh ? "localhost,127.0.0.1,.internal" : "localhost,127.0.0.1,.internal"}
                value={proxyBypass}
                onChange={(e) => setProxyBypass(e.target.value)}
              />
            </div>
          </div>
        </div>
      </div>

      {/* Log Retention & Config */}
      <div className="glass rounded-2xl p-6 space-y-5">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "其他配置" : "Other Configuration"}</h2>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          {/* Log Retention */}
          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div>
              <p className="text-sm font-medium text-slate-700">{isZh ? "日志保留" : "Log Retention"}</p>
              <p className="text-xs text-slate-500">
                {isZh ? "自动删除超过 N 天的日志" : "Auto-delete logs older than N days"}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Input
                type="number"
                min={1}
                max={365}
                value={retentionValue}
                onChange={(e) => setRetentionInput(e.target.value)}
                className="w-24"
              />
              <span className="text-sm text-slate-500">{isZh ? "天" : "days"}</span>
              <Button
                onClick={() => saveSetting.mutate(retentionValue)}
                disabled={saveSetting.isPending}
                size="sm"
                className="ml-auto flex items-center gap-1.5"
              >
                {saveSetting.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Save className="h-3.5 w-3.5" />
                )}
                {isZh ? "保存" : "Save"}
              </Button>
            </div>
            {saveSetting.isSuccess && (
              <p className="text-xs text-green-600">{isZh ? "保存成功" : "Saved successfully"}</p>
            )}
          </div>

          {/* Import / Export */}
          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div>
              <p className="text-sm font-medium text-slate-700">{isZh ? "配置备份" : "Config Backup"}</p>
              <p className="text-xs text-slate-500">
                {isZh ? "导出或导入提供商、路由和设置" : "Export or import providers, routes & settings"}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                onClick={() => exportMut.mutate()}
                disabled={exportMut.isPending}
                size="sm"
                className="flex items-center gap-1.5"
              >
                {exportMut.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="h-3.5 w-3.5" />
                )}
                {isZh ? "导出" : "Export"}
              </Button>
              <Button
                onClick={() => fileRef.current?.click()}
                disabled={importMut.isPending}
                variant="secondary"
                size="sm"
                className="flex items-center gap-1.5"
              >
                {importMut.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Upload className="h-3.5 w-3.5" />
                )}
                {isZh ? "导入" : "Import"}
              </Button>
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
                {isZh
                  ? `已导入：${(importMut.data as ImportResult).providers_imported} 个提供商，${(importMut.data as ImportResult).routes_imported} 条路由，${(importMut.data as ImportResult).settings_imported} 项设置`
                  : `Imported: ${(importMut.data as ImportResult).providers_imported} providers, ${(importMut.data as ImportResult).routes_imported} routes, ${(importMut.data as ImportResult).settings_imported} settings`}
              </p>
            )}
          </div>

        </div>
      </div>

      <ConfirmDialog
        open={Boolean(cacheToggleDialog)}
        onOpenChange={(open) => {
          if (!open) setCacheToggleDialog(null);
        }}
        title={
          cacheToggleDialog?.kind === "exact"
            ? (isZh ? "确认启用精确匹配缓存" : "Enable Exact Cache")
            : cacheToggleDialog?.kind === "semantic"
              ? (isZh ? "确认启用语义相似缓存" : "Enable Semantic Cache")
              : (isZh ? "无法启用语义相似缓存" : "Cannot Enable Semantic Cache")
        }
        description={
          cacheToggleDialog?.kind === "semantic_missing_route"
            ? (isZh
              ? "请先在“Embedding 路由（必选）”中选择一条向量路由，然后再启用语义相似缓存。"
              : "Please select an embedding route before enabling semantic cache.")
            : cacheToggleDialog?.kind === "exact"
              ? (isZh ? "确认后将立即保存并启用精确匹配缓存。" : "This will save and enable exact cache immediately.")
              : (isZh ? "确认后将立即保存并启用语义相似缓存。" : "This will save and enable semantic cache immediately.")
        }
        hideCancel={cacheToggleDialog?.kind === "semantic_missing_route"}
        cancelText={isZh ? "取消" : "Cancel"}
        confirmText={
          cacheToggleDialog?.kind === "semantic_missing_route"
            ? (isZh ? "关闭" : "Close")
            : (isZh ? "确认开启" : "Enable")
        }
        onConfirm={handleConfirmCacheToggle}
        confirmClassName="bg-slate-900 text-white hover:bg-slate-700"
      />

      <ConfirmDialog
        open={Boolean(proxyToggleDialog)}
        onOpenChange={(open) => {
          if (!open) setProxyToggleDialog(null);
        }}
        title={
          proxyToggleDialog?.kind === "missing_url"
            ? (isZh ? "无法启用代理" : "Cannot Enable Proxy")
            : proxyToggleDialog?.kind === "confirm" && proxyToggleDialog.nextEnabled
              ? (isZh ? "确认启用代理" : "Enable Proxy")
              : (isZh ? "确认关闭代理" : "Disable Proxy")
        }
        description={
          proxyToggleDialog?.kind === "missing_url"
            ? (isZh ? "请先填写代理 URL，再启用代理。" : "Please set proxy URL before enabling proxy.")
            : proxyToggleDialog?.kind === "confirm" && proxyToggleDialog.nextEnabled
              ? (isZh ? "确认后将立即保存并启用代理设置。" : "This will save and enable proxy settings immediately.")
              : (isZh ? "确认后将立即保存并关闭代理设置。" : "This will save and disable proxy settings immediately.")
        }
        hideCancel={proxyToggleDialog?.kind === "missing_url"}
        cancelText={isZh ? "取消" : "Cancel"}
        confirmText={
          proxyToggleDialog?.kind === "missing_url"
            ? (isZh ? "关闭" : "Close")
            : proxyToggleDialog?.kind === "confirm" && proxyToggleDialog.nextEnabled
              ? (isZh ? "确认开启" : "Enable")
              : (isZh ? "确认关闭" : "Disable")
        }
        onConfirm={handleConfirmProxyToggle}
        confirmClassName="bg-slate-900 text-white hover:bg-slate-700"
      />

      <ConfirmDialog
        open={Boolean(errorDialog)}
        onOpenChange={(open) => {
          if (!open) setErrorDialog(null);
        }}
        title={errorDialog?.title ?? ""}
        description={errorDialog?.description}
        hideCancel
        confirmText={isZh ? "我知道了" : "OK"}
        onConfirm={() => setErrorDialog(null)}
      />
    </div>
  );
}
