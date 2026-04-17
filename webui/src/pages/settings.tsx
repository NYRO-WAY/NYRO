import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, useRef } from "react";
import { backend, IS_TAURI } from "@/lib/backend";
import { localizeBackendErrorMessage } from "@/lib/backend-error";
import type {
  CacheSettings,
  ExportData,
  GatewayStatus,
  ImportResult,
  Route as RouteType,
} from "@/lib/types";
import { useLocale } from "@/lib/i18n";
import {
  Download,
  Upload,
  Save,
  Loader2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

function ToggleStatusLabel({ enabled, isZh }: { enabled: boolean; isZh: boolean }) {
  return (
    <Badge variant={enabled ? "success" : "secondary"} className="connect-label-badge">
      {enabled ? (isZh ? "已启用" : "Enabled") : (isZh ? "未启用" : "Disabled")}
    </Badge>
  );
}

export default function SettingsPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";
  const appVersion = import.meta.env.VITE_APP_VERSION;

  const qc = useQueryClient();
  const fileRef = useRef<HTMLInputElement>(null);
  const [errorDialog, setErrorDialog] = useState<{ title: string; description?: string } | null>(null);

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
  const retentionBaseline = (retentionDays ?? "30").trim();
  const retentionCurrent = retentionInput.trim();
  const retentionDirty = retentionCurrent !== retentionBaseline;
  const [proxyEnabled, setProxyEnabled] = useState(false);
  const [proxyUrl, setProxyUrl] = useState("");
  const [proxyBypass, setProxyBypass] = useState("");
  const [cacheForm, setCacheForm] = useState<CacheSettings>({
    exact: {
      enabled: false,
      default_ttl: 3600,
      max_entries: 1000,
      stream_replay_tps: 100,
      expose_headers: true,
    },
    semantic: {
      enabled: false,
      embedding_route: "",
      similarity_threshold: 0.92,
      vector_dimensions: 1536,
      default_ttl: 600,
      max_entries: 500,
      stream_replay_tps: 100,
      expose_headers: true,
    },
  });
  const embeddingRoutes = routes.filter((route) => route.route_type === "embedding");
  const selectedEmbeddingRouteExists = embeddingRoutes.some(
    (route) => route.virtual_model === cacheForm.semantic.embedding_route,
  );
  const exactCacheDirty = cacheSettings
    ? cacheForm.exact.enabled !== cacheSettings.exact.enabled
      || cacheForm.exact.default_ttl !== cacheSettings.exact.default_ttl
      || cacheForm.exact.max_entries !== cacheSettings.exact.max_entries
      || cacheForm.exact.stream_replay_tps !== cacheSettings.exact.stream_replay_tps
      || cacheForm.exact.expose_headers !== cacheSettings.exact.expose_headers
    : false;
  const semanticCacheDirty = cacheSettings
    ? cacheForm.semantic.enabled !== cacheSettings.semantic.enabled
      || cacheForm.semantic.embedding_route !== cacheSettings.semantic.embedding_route
      || cacheForm.semantic.similarity_threshold !== cacheSettings.semantic.similarity_threshold
      || cacheForm.semantic.vector_dimensions !== cacheSettings.semantic.vector_dimensions
      || cacheForm.semantic.default_ttl !== cacheSettings.semantic.default_ttl
      || cacheForm.semantic.max_entries !== cacheSettings.semantic.max_entries
      || cacheForm.semantic.stream_replay_tps !== cacheSettings.semantic.stream_replay_tps
      || cacheForm.semantic.expose_headers !== cacheSettings.semantic.expose_headers
    : false;
  const normalizedProxyEnabledSetting = ["1", "true", "yes", "on"].includes(
    (proxyEnabledSetting ?? "").trim().toLowerCase(),
  );
  const proxyDirty =
    proxyEnabled !== normalizedProxyEnabledSetting
    || proxyUrl.trim() !== (proxyUrlSetting ?? "").trim()
    || proxyBypass.trim() !== (proxyBypassSetting ?? "").trim();

  useEffect(() => {
    setProxyEnabled(normalizedProxyEnabledSetting);
    setProxyUrl(proxyUrlSetting ?? "");
    setProxyBypass(proxyBypassSetting ?? "");
  }, [normalizedProxyEnabledSetting, proxyUrlSetting, proxyBypassSetting]);

  useEffect(() => {
    setRetentionInput(retentionDays ?? "30");
  }, [retentionDays]);

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
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["setting", "proxy_enabled"] });
      qc.invalidateQueries({ queryKey: ["setting", "proxy_url"] });
      qc.invalidateQueries({ queryKey: ["setting", "proxy_bypass"] });
    },
    onError: (error: unknown) => {
      showErrorDialog("保存代理设置失败", "Failed to save proxy settings", error);
    },
  });
  const saveCacheMut = useMutation({
    mutationFn: (next: CacheSettings) => backend("update_cache_settings", { input: next }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["cache-settings"] });
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

  function handleSaveExactCache() {
    const next = cacheForm;
    saveCacheMut.mutate(next, {
      onSuccess: () => {
        qc.setQueryData(["cache-settings"], next);
      },
      onError: (error: unknown) => {
        showErrorDialog("保存精确匹配缓存设置失败", "Failed to save exact cache settings", error);
      },
    });
  }

  function handleSaveSemanticCache() {
    const hasSelectedEmbeddingRoute =
      cacheForm.semantic.embedding_route.trim().length > 0 && selectedEmbeddingRouteExists;
    if (cacheForm.semantic.enabled && !hasSelectedEmbeddingRoute) {
      setErrorDialog({
        title: isZh ? "无法保存语义相似缓存" : "Cannot Save Semantic Cache",
        description: isZh
          ? "请先在“Embedding 路由（必选）”中选择一条向量路由，然后再保存语义相似缓存配置。"
          : "Please select an embedding route before saving semantic cache settings.",
      });
      return;
    }

    const next = cacheForm;
    saveCacheMut.mutate(next, {
      onSuccess: () => {
        qc.setQueryData(["cache-settings"], next);
      },
      onError: (error: unknown) => {
        showErrorDialog("保存语义相似缓存设置失败", "Failed to save semantic cache settings", error);
      },
    });
  }

  function handleSaveProxy() {
    const url = proxyUrl.trim();
    const bypass = proxyBypass.trim();
    if (proxyEnabled && !url) {
      setErrorDialog({
        title: isZh ? "无法保存代理配置" : "Cannot Save Proxy Settings",
        description: isZh ? "请先填写代理 URL，再保存代理配置。" : "Please set proxy URL before saving proxy settings.",
      });
      return;
    }
    saveProxyToggle.mutate(
      {
        enabled: proxyEnabled,
        url,
        bypass,
      },
      {
        onSuccess: () => {
          setProxyUrl(url);
          setProxyBypass(bypass);
          qc.setQueryData(["setting", "proxy_enabled"], proxyEnabled ? "true" : "false");
          qc.setQueryData(["setting", "proxy_url"], url);
          qc.setQueryData(["setting", "proxy_bypass"], bypass);
        },
      },
    );
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
            <div className="space-y-1.5">
              <label className="ml-1 text-xs text-slate-700">
                {isZh ? "精确匹配缓存" : "Exact cache"}
              </label>
              <div className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2.5">
                <div className="flex items-center gap-2">
                  <ToggleStatusLabel enabled={cacheForm.exact.enabled} isZh={isZh} />
                </div>
                <Switch
                  checked={cacheForm.exact.enabled}
                  disabled={saveCacheMut.isPending}
                  onCheckedChange={(checked) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      exact: { ...prev.exact, enabled: checked },
                    }))
                  }
                />
              </div>
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
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "流式重放速率 (TPS)" : "Stream Replay TPS"}</label>
                <Input
                  type="number"
                  min={0}
                  placeholder={isZh ? "0 = 不限速" : "0 = No Limit"}
                  value={cacheForm.exact.stream_replay_tps}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      exact: { ...prev.exact, stream_replay_tps: Math.max(0, Number(e.target.value || 0)) },
                    }))
                  }
                />
              </div>
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "输出缓存响应头" : "Expose Cache Headers"}</label>
                <div className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2.5">
                  <div className="flex items-center gap-2">
                    <ToggleStatusLabel enabled={cacheForm.exact.expose_headers} isZh={isZh} />
                  </div>
                  <Switch
                    checked={cacheForm.exact.expose_headers}
                    disabled={saveCacheMut.isPending}
                    onCheckedChange={(checked) =>
                      setCacheForm((prev) => ({
                        ...prev,
                        exact: { ...prev.exact, expose_headers: checked },
                      }))
                    }
                  />
                </div>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                onClick={handleSaveExactCache}
                disabled={saveCacheMut.isPending || !exactCacheDirty}
                size="sm"
                className="flex items-center gap-1.5"
              >
                {saveCacheMut.isPending ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Save className="h-3.5 w-3.5" />
                )}
                {isZh ? "保存" : "Save"}
              </Button>
              {exactCacheDirty && (
                <p className="text-xs text-amber-600">
                  {isZh ? "配置已修改，保存后生效" : "Unsaved changes, save to apply"}
                </p>
              )}
            </div>
          </div>

          <div className="rounded-xl bg-slate-50 p-4 space-y-3">
            <div className="space-y-1.5">
              <label className="ml-1 text-xs text-slate-700">
                {isZh ? "语义相似缓存" : "Semantic cache"}
              </label>
              <div className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2.5">
                <div className="flex items-center gap-2">
                  <ToggleStatusLabel enabled={cacheForm.semantic.enabled} isZh={isZh} />
                </div>
                <Switch
                  checked={cacheForm.semantic.enabled}
                  disabled={saveCacheMut.isPending}
                  onCheckedChange={(checked) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      semantic: {
                        ...prev.semantic,
                        enabled: checked,
                        embedding_route: checked ? prev.semantic.embedding_route : "",
                      },
                    }))
                  }
                />
              </div>
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
                        {route.name} · {route.virtual_model}
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
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "流式重放速率 (TPS)" : "Stream Replay TPS"}</label>
                <Input
                  type="number"
                  min={0}
                  placeholder={isZh ? "0 = 不限速" : "0 = No Limit"}
                  value={cacheForm.semantic.stream_replay_tps}
                  onChange={(e) =>
                    setCacheForm((prev) => ({
                      ...prev,
                      semantic: { ...prev.semantic, stream_replay_tps: Math.max(0, Number(e.target.value || 0)) },
                    }))
                  }
                />
              </div>
              <div className="space-y-1.5">
                <label className="ml-1 text-xs text-slate-700">{isZh ? "输出缓存响应头" : "Expose Cache Headers"}</label>
                <div className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2.5">
                  <div className="flex items-center gap-2">
                    <ToggleStatusLabel enabled={cacheForm.semantic.expose_headers} isZh={isZh} />
                  </div>
                  <Switch
                    checked={cacheForm.semantic.expose_headers}
                    disabled={saveCacheMut.isPending}
                    onCheckedChange={(checked) =>
                      setCacheForm((prev) => ({
                        ...prev,
                        semantic: { ...prev.semantic, expose_headers: checked },
                      }))
                    }
                  />
                </div>
              </div>
              </div>
              <div className="flex items-center gap-2">
                <Button
                  onClick={handleSaveSemanticCache}
                  disabled={saveCacheMut.isPending || detectEmbeddingDimensionsMut.isPending || !semanticCacheDirty}
                  size="sm"
                  className="flex items-center gap-1.5"
                >
                  {saveCacheMut.isPending ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Save className="h-3.5 w-3.5" />
                  )}
                  {isZh ? "保存" : "Save"}
                </Button>
                {semanticCacheDirty && (
                  <p className="text-xs text-amber-600">
                    {isZh ? "配置已修改，保存后生效" : "Unsaved changes, save to apply"}
                  </p>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="glass rounded-2xl p-6 space-y-5">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "代理配置" : "Proxy Configuration"}</h2>
        <div className="rounded-xl bg-slate-50 p-4 space-y-3">
          <div className="space-y-1.5">
            <label className="ml-1 text-xs text-slate-700">{isZh ? "代理" : "Proxy"}</label>
            <div className="flex items-center justify-between rounded-lg border border-slate-200 bg-white px-3 py-2.5">
              <div className="flex items-center gap-2">
                <ToggleStatusLabel enabled={proxyEnabled} isZh={isZh} />
              </div>
              <Switch
                checked={proxyEnabled}
                disabled={saveProxyToggle.isPending}
                onCheckedChange={setProxyEnabled}
              />
            </div>
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
          <div className="flex items-center gap-2">
            <Button
              onClick={handleSaveProxy}
              disabled={saveProxyToggle.isPending || !proxyDirty}
              size="sm"
              className="flex items-center gap-1.5"
            >
              {saveProxyToggle.isPending ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Save className="h-3.5 w-3.5" />
              )}
              {isZh ? "保存" : "Save"}
            </Button>
            {proxyDirty && (
              <p className="text-xs text-amber-600">
                {isZh ? "配置已修改，保存后生效" : "Unsaved changes, save to apply"}
              </p>
            )}
          </div>
        </div>
      </div>

      {/* Log Retention & Config */}
      <div className="glass rounded-2xl p-6 space-y-5">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "其他配置" : "Other Configuration"}</h2>

        <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
          {/* Log Retention */}
          <div className="rounded-xl bg-slate-50 p-4">
            <div className="flex h-full flex-col">
              <div className="space-y-3">
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
                    value={retentionInput}
                    onChange={(e) => setRetentionInput(e.target.value)}
                    className="w-24"
                  />
                  <span className="text-sm text-slate-500">{isZh ? "天" : "days"}</span>
                  <Button
                    onClick={() => saveSetting.mutate(retentionInput)}
                    disabled={saveSetting.isPending || !retentionDirty}
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
              </div>
              <div className="mt-1 min-h-[0.875rem]">
                {retentionDirty ? (
                  <p className="text-xs text-amber-600">
                    {isZh ? "配置已修改，保存后生效" : "Unsaved changes, save to apply"}
                  </p>
                ) : saveSetting.isSuccess ? (
                  <p className="text-xs text-green-600">{isZh ? "保存成功" : "Saved successfully"}</p>
                ) : null}
              </div>
            </div>
          </div>

          {/* Import / Export */}
          <div className="rounded-xl bg-slate-50 p-4">
            <div className="flex h-full flex-col">
              <div className="space-y-3">
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
              </div>
              <div className="mt-2 min-h-4">
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

        </div>
      </div>

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
