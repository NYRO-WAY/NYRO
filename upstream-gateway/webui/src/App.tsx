import { useDeferredValue, useEffect, useMemo, useState } from "react";
import {
  Activity,
  CirclePlus,
  KeyRound,
  LoaderCircle,
  Moon,
  RefreshCw,
  Save,
  Server,
  ShieldCheck,
  Sun,
  Trash2,
  Upload,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { NyroTextField, NyroTextareaField } from "@/components/ui/nyro-fields";
import {
  ApiError,
  type GatewayKey,
  type GatewayModelRule,
  type GatewayProviderBundle,
  type GatewayProviderSummary,
  type GatewayAuthStrategy,
  type ProviderRuntimeView,
  deleteProviderBundle,
  getAdminHealth,
  getProviderBundle,
  getProviderRuntime,
  listProviders,
  listRuntimeProviders,
  saveProviderBundle,
} from "@/lib/api";
import {
  buildRouteHint,
  cn,
  formatNumber,
  formatPercent,
  parseOptionalNumber,
  toInputValue,
} from "@/lib/utils";

type ViewMode = "providers" | "runtime";
type ThemeMode = "light" | "dark";

const DEFAULT_BUNDLE = (): GatewayProviderBundle => ({
  provider: {
    id: "",
    name: "",
    vendor: "gemini",
    base_url: "",
    auth_strategy: {
      kind: "query_api_key",
      parameter_name: "key",
    },
    enabled: true,
  },
  keys: [
    {
      id: "",
      provider_id: "",
      display_name: "",
      api_key: "",
      enabled: true,
      weight: 1,
    },
  ],
  model_rules: [
    {
      provider_id: "",
      model: "*",
      rpm: null,
      rpd: null,
      tpm: null,
      tpm_mode: null,
      tokenizer_encoding: null,
      tokenizer_model: "",
    },
  ],
  daily_reset: {
    timezone: "+08:00",
    hour: 4,
    minute: 0,
  },
});

function defaultAuthStrategyForVendor(
  vendor: GatewayProviderBundle["provider"]["vendor"],
): GatewayAuthStrategy {
  if (vendor === "anthropic") {
    return {
      kind: "header_api_key",
      header_name: "x-api-key",
    };
  }
  if (vendor === "gemini") {
    return {
      kind: "query_api_key",
      parameter_name: "key",
    };
  }
  return { kind: "bearer" };
}

function normalizeAuthStrategy(strategy: GatewayAuthStrategy): GatewayAuthStrategy {
  if (strategy.kind === "header_api_key") {
    return {
      kind: "header_api_key",
      header_name: strategy.header_name.trim() || "x-api-key",
    };
  }
  if (strategy.kind === "query_api_key") {
    return {
      kind: "query_api_key",
      parameter_name: strategy.parameter_name.trim() || "key",
    };
  }
  return { kind: "bearer" };
}

function normalizeBundle(bundle: GatewayProviderBundle): GatewayProviderBundle {
  const providerId = bundle.provider.id.trim();
  return {
    provider: {
      ...bundle.provider,
      id: providerId,
      name: bundle.provider.name.trim(),
      base_url: bundle.provider.base_url.trim(),
      auth_strategy: normalizeAuthStrategy(bundle.provider.auth_strategy),
    },
    keys: bundle.keys.map((key) => ({
      ...key,
      id: key.id.trim(),
      provider_id: providerId,
      display_name: key.display_name?.trim() || null,
      api_key: key.api_key.trim(),
      weight: key.weight && key.weight > 0 ? key.weight : 1,
    })),
    model_rules: bundle.model_rules.map((rule) => ({
      ...rule,
      provider_id: providerId,
      model: rule.model.trim(),
      tokenizer_model: rule.tokenizer_model?.trim() || null,
    })),
    daily_reset: {
      timezone: bundle.daily_reset.timezone.trim(),
      hour: bundle.daily_reset.hour,
      minute: bundle.daily_reset.minute,
    },
  };
}

function createKeyDraft(providerId: string): GatewayKey {
  return {
    id: "",
    provider_id: providerId,
    display_name: "",
    api_key: "",
    enabled: true,
    weight: 1,
  };
}

function buildUniqueKeyId(providerId: string, usedIds: Set<string>) {
  const prefix = providerId.trim() || "key";
  let counter = usedIds.size + 1;
  let candidate = `${prefix}-key-${counter}`;
  while (usedIds.has(candidate)) {
    counter += 1;
    candidate = `${prefix}-key-${counter}`;
  }
  usedIds.add(candidate);
  return candidate;
}

function parseBulkKeys(
  raw: string,
  providerId: string,
  existingKeys: GatewayKey[],
): { keys: GatewayKey[]; skipped: number } {
  const usedIds = new Set(existingKeys.map((key) => key.id).filter(Boolean));
  const lines = raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  const parsed: GatewayKey[] = [];
  let skipped = 0;

  for (const line of lines) {
    const parts =
      line.includes("\t")
        ? line.split("\t")
        : line.includes("|")
          ? line.split("|")
          : line.split(",");
    const normalized = parts.map((part) => part.trim());
    const [apiKey = "", displayName = "", weightRaw = "", enabledRaw = ""] = normalized;

    if (!apiKey) {
      skipped += 1;
      continue;
    }

    const weight = parseOptionalNumber(weightRaw);
    const enabled =
      enabledRaw.trim() === ""
        ? true
        : ["1", "true", "yes", "enabled", "on", "启用"].includes(enabledRaw.toLowerCase());

    parsed.push({
      id: buildUniqueKeyId(providerId, usedIds),
      provider_id: providerId,
      display_name: displayName || null,
      api_key: apiKey,
      enabled,
      weight: weight && weight > 0 ? weight : 1,
    });
  }

  return { keys: parsed, skipped };
}

function createRuleDraft(providerId: string): GatewayModelRule {
  return {
    provider_id: providerId,
    model: "*",
    rpm: null,
    rpd: null,
    tpm: null,
    tpm_mode: null,
    tokenizer_encoding: null,
    tokenizer_model: "",
  };
}

function getAuthParam(strategy: GatewayAuthStrategy) {
  if (strategy.kind === "header_api_key") {
    return strategy.header_name;
  }
  if (strategy.kind === "query_api_key") {
    return strategy.parameter_name;
  }
  return "";
}

function setAuthKind(
  bundle: GatewayProviderBundle,
  kind: GatewayAuthStrategy["kind"],
): GatewayProviderBundle {
  if (kind === "bearer") {
    return {
      ...bundle,
      provider: {
        ...bundle.provider,
        auth_strategy: { kind: "bearer" },
      },
    };
  }
  if (kind === "header_api_key") {
    return {
      ...bundle,
      provider: {
        ...bundle.provider,
        auth_strategy: {
          kind: "header_api_key",
          header_name:
            bundle.provider.auth_strategy.kind === "header_api_key"
              ? bundle.provider.auth_strategy.header_name
              : "x-api-key",
        },
      },
    };
  }
  return {
    ...bundle,
    provider: {
      ...bundle.provider,
      auth_strategy: {
        kind: "query_api_key",
        parameter_name:
          bundle.provider.auth_strategy.kind === "query_api_key"
            ? bundle.provider.auth_strategy.parameter_name
            : "key",
      },
    },
  };
}

export function App() {
  const [activeView, setActiveView] = useState<ViewMode>("providers");
  const [theme, setTheme] = useState<ThemeMode>("light");
  const [healthText, setHealthText] = useState("检查中");
  const [healthDetail, setHealthDetail] = useState("正在读取控制面健康状态。");
  const [healthOk, setHealthOk] = useState<boolean | null>(null);
  const [providers, setProviders] = useState<GatewayProviderSummary[]>([]);
  const [runtimeViews, setRuntimeViews] = useState<ProviderRuntimeView[]>([]);
  const [draft, setDraft] = useState<GatewayProviderBundle>(DEFAULT_BUNDLE());
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
  const [selectedKeyIndex, setSelectedKeyIndex] = useState(0);
  const [searchText, setSearchText] = useState("");
  const [keySearchText, setKeySearchText] = useState("");
  const [batchKeysText, setBatchKeysText] = useState("");
  const [busy, setBusy] = useState(false);
  const [runtimeBusy, setRuntimeBusy] = useState(false);
  const [alert, setAlert] = useState<{ tone: "success" | "error" | "info"; text: string } | null>(
    null,
  );

  const deferredSearch = useDeferredValue(searchText);
  const deferredKeySearch = useDeferredValue(keySearchText);

  const visibleProviders = providers.filter((provider) => {
    const query = deferredSearch.trim().toLowerCase();
    if (!query) {
      return true;
    }
    return (
      provider.id.toLowerCase().includes(query) ||
      provider.name.toLowerCase().includes(query) ||
      provider.vendor.toLowerCase().includes(query)
    );
  });

  const visibleKeyEntries = useMemo(() => {
    const query = deferredKeySearch.trim().toLowerCase();
    return draft.keys
      .map((key, index) => ({ key, index }))
      .filter(({ key }) => {
        if (!query) {
          return true;
        }
        return (
          (key.id ?? "").toLowerCase().includes(query) ||
          (key.display_name ?? "").toLowerCase().includes(query) ||
          key.api_key.toLowerCase().includes(query)
        );
      });
  }, [deferredKeySearch, draft.keys]);

  const selectedKey = draft.keys[selectedKeyIndex] ?? null;

  useEffect(() => {
    const saved = window.localStorage.getItem("nyro-theme");
    const initial =
      saved === "dark" || saved === "light"
        ? saved
        : window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light";
    setTheme(initial);
    document.documentElement.setAttribute("data-theme", initial);
  }, []);

  useEffect(() => {
    void bootstrap();
  }, []);

  useEffect(() => {
    if (draft.keys.length === 0) {
      setSelectedKeyIndex(0);
      return;
    }
    if (selectedKeyIndex > draft.keys.length - 1) {
      setSelectedKeyIndex(draft.keys.length - 1);
    }
  }, [draft.keys.length, selectedKeyIndex]);

  async function bootstrap() {
    await Promise.all([refreshHealth(), refreshProviderList(), refreshRuntime()]);
  }

  async function refreshHealth() {
    try {
      const health = await getAdminHealth();
      setHealthOk(true);
      setHealthText("运行正常");
      setHealthDetail(`控制面监听地址：${health.admin_bind_addr}`);
    } catch (error) {
      setHealthOk(false);
      setHealthText("连接失败");
      setHealthDetail(readErrorMessage(error));
    }
  }

  async function refreshProviderList(nextSelectedId?: string | null) {
    setBusy(true);
    try {
      const items = await listProviders();
      setProviders(items);
      const candidate =
        nextSelectedId ??
        selectedProviderId ??
        (items.length > 0 ? items[0].id : null);
      if (candidate) {
        await selectProvider(candidate, items);
      } else {
        setSelectedProviderId(null);
        setDraft(DEFAULT_BUNDLE());
        setSelectedKeyIndex(0);
      }
    } catch (error) {
      setAlert({
        tone: "error",
        text: readErrorMessage(error),
      });
    } finally {
      setBusy(false);
    }
  }

  async function refreshRuntime() {
    setRuntimeBusy(true);
    try {
      const items = await listRuntimeProviders();
      setRuntimeViews(items);
    } catch (error) {
      setAlert({
        tone: "error",
        text: readErrorMessage(error),
      });
    } finally {
      setRuntimeBusy(false);
    }
  }

  async function selectProvider(providerId: string, summaries?: GatewayProviderSummary[]) {
    setBusy(true);
    try {
      const bundle = await getProviderBundle(providerId);
      setSelectedProviderId(providerId);
      setDraft({
        ...bundle,
        keys: bundle.keys.map((key) => ({
          ...key,
          display_name: key.display_name ?? "",
        })),
        model_rules: bundle.model_rules.map((rule) => ({
          ...rule,
          tokenizer_model: rule.tokenizer_model ?? "",
        })),
      });
      setSelectedKeyIndex(0);
      setKeySearchText("");
      setBatchKeysText("");
      if (summaries) {
        setProviders(summaries);
      }
    } catch (error) {
      setAlert({
        tone: "error",
        text: readErrorMessage(error),
      });
    } finally {
      setBusy(false);
    }
  }

  function resetDraft() {
    setSelectedProviderId(null);
    setDraft(DEFAULT_BUNDLE());
    setSelectedKeyIndex(0);
    setKeySearchText("");
    setBatchKeysText("");
    setAlert({
      tone: "info",
      text: "已切换到新建模式。填写后保存即可创建新的上游提供商。",
    });
  }

  async function saveDraft() {
    const normalized = normalizeBundle(draft);
    if (!normalized.provider.id) {
      setAlert({ tone: "error", text: "Provider ID 不能为空。" });
      return;
    }
    setBusy(true);
    try {
      const saved = await saveProviderBundle(normalized.provider.id, normalized);
      setSelectedProviderId(saved.provider.id);
      setDraft({
        ...saved,
        keys: saved.keys.map((key) => ({
          ...key,
          display_name: key.display_name ?? "",
        })),
        model_rules: saved.model_rules.map((rule) => ({
          ...rule,
          tokenizer_model: rule.tokenizer_model ?? "",
        })),
      });
      setSelectedKeyIndex(0);
      setBatchKeysText("");
      setAlert({ tone: "success", text: `已保存 Provider：${saved.provider.id}` });
      await Promise.all([refreshProviderList(saved.provider.id), refreshRuntime(), refreshHealth()]);
    } catch (error) {
      setAlert({ tone: "error", text: readErrorMessage(error) });
    } finally {
      setBusy(false);
    }
  }

  async function deleteCurrentProvider() {
    if (!selectedProviderId) {
      setAlert({ tone: "info", text: "当前是新建草稿，还没有可删除的 Provider。" });
      return;
    }
    if (!window.confirm(`确认删除 Provider “${selectedProviderId}” 吗？`)) {
      return;
    }
    setBusy(true);
    try {
      await deleteProviderBundle(selectedProviderId);
      setAlert({ tone: "success", text: `已删除 Provider：${selectedProviderId}` });
      setSelectedProviderId(null);
      setDraft(DEFAULT_BUNDLE());
      setSelectedKeyIndex(0);
      setKeySearchText("");
      setBatchKeysText("");
      await Promise.all([refreshProviderList(null), refreshRuntime(), refreshHealth()]);
    } catch (error) {
      setAlert({ tone: "error", text: readErrorMessage(error) });
    } finally {
      setBusy(false);
    }
  }

  async function loadSelectedRuntime() {
    if (!selectedProviderId) {
      setAlert({ tone: "info", text: "请先选择一个 Provider。" });
      return;
    }
    setRuntimeBusy(true);
    try {
      const view = await getProviderRuntime(selectedProviderId);
      setRuntimeViews((current) => {
        const next = current.filter((item) => item.summary.id !== view.summary.id);
        return [view, ...next];
      });
      setActiveView("runtime");
      setAlert({ tone: "success", text: `已加载 ${selectedProviderId} 的运行态快照。` });
    } catch (error) {
      setAlert({ tone: "error", text: readErrorMessage(error) });
    } finally {
      setRuntimeBusy(false);
    }
  }

  function toggleTheme() {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    window.localStorage.setItem("nyro-theme", next);
    document.documentElement.setAttribute("data-theme", next);
  }

  const preview = JSON.stringify(normalizeBundle(draft), null, 2);
  const enabledKeyCount = draft.keys.filter((key) => key.enabled).length;
  const authParamLabel =
    draft.provider.auth_strategy.kind === "header_api_key"
      ? "Header 名称"
      : draft.provider.auth_strategy.kind === "query_api_key"
        ? "Query 参数名"
        : "Bearer 无需额外参数";

  function updateSelectedKey(
    updater: (key: GatewayKey) => GatewayKey,
  ) {
    if (selectedKeyIndex < 0 || selectedKeyIndex >= draft.keys.length) {
      return;
    }
    setDraft((current) => ({
      ...current,
      keys: current.keys.map((item, itemIndex) =>
        itemIndex === selectedKeyIndex ? updater(item) : item,
      ),
    }));
  }

  function addSingleKey() {
    setDraft((current) => ({
      ...current,
      keys: [...current.keys, createKeyDraft(current.provider.id)],
    }));
    setSelectedKeyIndex(draft.keys.length);
  }

  function removeKeyAt(index: number) {
    setDraft((current) => ({
      ...current,
      keys: current.keys.length === 1 ? current.keys : current.keys.filter((_, itemIndex) => itemIndex !== index),
    }));
    setSelectedKeyIndex((currentIndex) => {
      if (draft.keys.length === 1) {
        return currentIndex;
      }
      if (currentIndex > index) {
        return currentIndex - 1;
      }
      return Math.max(0, Math.min(currentIndex, draft.keys.length - 2));
    });
  }

  function importBatchKeys(mode: "append" | "replace") {
    const providerId = draft.provider.id.trim();
    const { keys, skipped } = parseBulkKeys(batchKeysText, providerId, mode === "append" ? draft.keys : []);
    if (keys.length === 0) {
      setAlert({
        tone: "error",
        text: skipped > 0 ? "没有成功解析任何 Key，请检查粘贴格式。" : "请先粘贴要导入的 Key。",
      });
      return;
    }

    setDraft((current) => ({
      ...current,
      keys: mode === "append" ? [...current.keys, ...keys] : keys,
    }));
    setSelectedKeyIndex(mode === "append" ? draft.keys.length : 0);
    setBatchKeysText("");
    setAlert({
      tone: "success",
      text:
        mode === "append"
          ? `已追加导入 ${keys.length} 个 Key${skipped > 0 ? `，跳过 ${skipped} 行` : ""}。`
          : `已覆盖导入 ${keys.length} 个 Key${skipped > 0 ? `，跳过 ${skipped} 行` : ""}。`,
    });
  }

  return (
    <div className="app-shell h-full bg-background">
      <div className="mx-auto flex h-full max-w-[1560px] gap-3 px-3 py-3 md:gap-4 md:px-4">
        <aside
          className={cn(
            "glass hidden h-full w-[15rem] shrink-0 flex-col overflow-hidden rounded-[1.5rem] lg:flex",
          )}
        >
          <div className="border-b border-white/60 px-5 py-5">
            <p className="text-[11px] font-semibold tracking-[0.2em] text-slate-500 uppercase">
              upstream-gateway
            </p>
            <h1 className="mt-2 text-xl font-semibold text-slate-900 dark:text-slate-50">
              中文管理台
            </h1>
            <p className="mt-2 text-sm leading-6 text-slate-500 dark:text-slate-300">
              沿用 Nyro WebUI 的玻璃质感和控制台布局，专门管理上游 Provider、Key 池和限流运行态。
            </p>
          </div>

          <nav className="flex-1 space-y-2 px-3 py-3">
            <SidebarButton
              active={activeView === "providers"}
              icon={<Server className="h-4 w-4" />}
              label="上游提供商"
              onClick={() => setActiveView("providers")}
            />
            <SidebarButton
              active={activeView === "runtime"}
              icon={<Activity className="h-4 w-4" />}
              label="运行态"
              onClick={() => setActiveView("runtime")}
            />
          </nav>

          <div className="border-t border-white/60 p-4">
            <div className="rounded-2xl border border-white/65 bg-white/65 p-4 dark:border-white/10 dark:bg-white/5">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="text-xs font-semibold tracking-[0.18em] text-slate-500 uppercase">
                    控制面状态
                  </p>
                  <p className="mt-2 text-sm font-medium text-slate-900 dark:text-slate-100">
                    {healthText}
                  </p>
                </div>
                <Badge variant={healthOk === false ? "danger" : healthOk ? "success" : "warning"}>
                  {healthOk === false ? "异常" : healthOk ? "在线" : "检查中"}
                </Badge>
              </div>
              <p className="mt-3 text-xs leading-5 text-slate-500 dark:text-slate-300">
                {healthDetail}
              </p>
            </div>
          </div>
        </aside>

        <main className="content-surface glass flex min-w-0 flex-1 flex-col overflow-hidden rounded-[1.5rem]">
          <header className="flex flex-wrap items-center justify-between gap-3 border-b border-white/60 px-4 py-3 md:px-5">
            <div>
              <p className="text-[11px] font-semibold tracking-[0.2em] text-slate-500 uppercase">
                {activeView === "providers" ? "Provider Control" : "Runtime Snapshot"}
              </p>
              <h2 className="mt-1 text-lg font-semibold text-slate-900 dark:text-slate-50">
                {activeView === "providers" ? "上游 Provider 管理" : "实时限流运行态"}
              </h2>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <Button variant="outline" size="icon" onClick={toggleTheme} title="切换明暗主题">
                {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
              </Button>
              <Button variant="outline" onClick={() => void refreshHealth()}>
                <ShieldCheck className="h-4 w-4" />
                重新检查
              </Button>
              <Button
                variant="outline"
                onClick={() => void (activeView === "providers" ? refreshProviderList() : refreshRuntime())}
              >
                <RefreshCw className="h-4 w-4" />
                全量刷新
              </Button>
            </div>
          </header>

          {alert && (
            <div
              className={cn(
                "mx-4 mt-4 rounded-2xl border px-4 py-3 text-sm md:mx-5",
                alert.tone === "error"
                  ? "border-red-200 bg-red-50 text-red-700"
                  : alert.tone === "success"
                    ? "border-green-200 bg-green-50 text-green-700"
                    : "border-slate-200 bg-slate-50 text-slate-700",
              )}
            >
              {alert.text}
            </div>
          )}

          <section className="min-h-0 flex-1 overflow-auto px-4 py-4 md:px-5">
            {activeView === "providers" ? (
              <div className="grid gap-4 xl:grid-cols-[320px_minmax(0,1fr)]">
                <section className="glass rounded-[1.25rem] p-4">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="text-xs font-semibold tracking-[0.18em] text-slate-500 uppercase">
                        列表
                      </p>
                      <h3 className="mt-1 text-base font-semibold text-slate-900 dark:text-slate-50">
                        已注册 Provider
                      </h3>
                    </div>
                    <Button variant="outline" size="icon" onClick={resetDraft}>
                      <CirclePlus className="h-4 w-4" />
                    </Button>
                  </div>

                  <div className="mt-4">
                    <NyroTextField
                      label="搜索 Provider"
                      value={searchText}
                      onChange={(event) => setSearchText(event.target.value)}
                    />
                  </div>

                  <div className="mt-4 space-y-3">
                    <Button className="w-full" onClick={resetDraft}>
                      <CirclePlus className="h-4 w-4" />
                      新建 Provider
                    </Button>
                    <Button className="w-full" variant="outline" onClick={() => void refreshProviderList()}>
                      <RefreshCw className="h-4 w-4" />
                      刷新列表
                    </Button>
                  </div>

                  <div className="mt-4 space-y-3">
                    {busy && (
                      <div className="flex items-center gap-2 rounded-2xl border border-dashed border-slate-200 px-4 py-5 text-sm text-slate-500">
                        <LoaderCircle className="h-4 w-4 animate-spin" />
                        正在加载 Provider...
                      </div>
                    )}

                    {!busy && visibleProviders.length === 0 && (
                      <div className="rounded-2xl border border-dashed border-slate-200 px-4 py-8 text-center text-sm text-slate-500">
                        没有匹配的 Provider。
                      </div>
                    )}

                    {!busy &&
                      visibleProviders.map((provider) => (
                        <button
                          key={provider.id}
                          type="button"
                          onClick={() => void selectProvider(provider.id)}
                          className={cn(
                            "w-full rounded-[1.1rem] border p-4 text-left transition hover:-translate-y-0.5",
                            selectedProviderId === provider.id
                              ? "border-slate-900 bg-slate-900 text-white shadow-[0_10px_24px_rgba(15,23,42,0.18)]"
                              : "border-white/65 bg-white/70 text-slate-900 hover:bg-white",
                          )}
                        >
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0">
                              <h4 className="truncate text-sm font-semibold">{provider.name || provider.id}</h4>
                              <p
                                className={cn(
                                  "mt-1 truncate text-xs",
                                  selectedProviderId === provider.id ? "text-white/70" : "text-slate-500",
                                )}
                              >
                                {provider.id}
                              </p>
                            </div>
                            <Badge
                              variant={
                                provider.enabled ? "success" : "warning"
                              }
                            >
                              {provider.enabled ? "启用" : "停用"}
                            </Badge>
                          </div>
                          <div className="mt-3 flex flex-wrap gap-2">
                            <Badge variant="outline">{provider.vendor}</Badge>
                            <Badge variant="outline">{provider.rate_limit.model_rule_count} 条规则</Badge>
                            <Badge variant="outline">{provider.rate_limit.enabled_key_count} 个启用 Key</Badge>
                          </div>
                        </button>
                      ))}
                  </div>
                </section>

                <section className="glass rounded-[1.25rem] p-4 md:p-5">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <p className="text-xs font-semibold tracking-[0.18em] text-slate-500 uppercase">
                        编辑器
                      </p>
                      <h3 className="mt-1 text-base font-semibold text-slate-900 dark:text-slate-50">
                        {selectedProviderId ? `编辑 ${selectedProviderId}` : "创建新的 Provider Bundle"}
                      </h3>
                      <p className="mt-2 text-sm leading-6 text-slate-500 dark:text-slate-300">
                        这里保存的是完整 Bundle：Provider 基础信息、Key 池、模型规则和日额度重置时间会一次性原子更新。
                      </p>
                    </div>

                    <div className="flex flex-wrap gap-2">
                      <Button variant="outline" onClick={() => void loadSelectedRuntime()}>
                        <Activity className="h-4 w-4" />
                        查看运行态
                      </Button>
                      <Button variant="danger" onClick={() => void deleteCurrentProvider()}>
                        <Trash2 className="h-4 w-4" />
                        删除
                      </Button>
                      <Button onClick={() => void saveDraft()}>
                        <Save className="h-4 w-4" />
                        保存 Provider
                      </Button>
                    </div>
                  </div>

                  <div className="mt-5 grid gap-4">
                    <section className="rounded-[1.2rem] border border-white/70 bg-white/55 p-4 dark:border-white/10 dark:bg-white/5">
                      <div className="mb-4 flex items-center justify-between gap-3">
                        <div>
                          <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-50">Provider 基础信息</h4>
                          <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                            路由提示：<code>{buildRouteHint(draft.provider.vendor, draft.provider.id)}</code>
                          </p>
                        </div>
                        <Badge variant={draft.provider.enabled ? "success" : "warning"}>
                          {draft.provider.enabled ? "已启用" : "已停用"}
                        </Badge>
                      </div>

                      <div className="grid gap-3 md:grid-cols-2">
                        <NyroTextField
                          label="Provider ID"
                          value={draft.provider.id}
                          onChange={(event) =>
                            setDraft((current) => ({
                              ...current,
                              provider: { ...current.provider, id: event.target.value },
                            }))
                          }
                        />
                        <NyroTextField
                          label="展示名称"
                          value={draft.provider.name}
                          onChange={(event) =>
                            setDraft((current) => ({
                              ...current,
                              provider: { ...current.provider, name: event.target.value },
                            }))
                          }
                        />
                        <label className="grid gap-2 text-sm font-medium text-slate-600 dark:text-slate-300">
                          <span>厂商类型</span>
                          <select
                            className="h-11 rounded-xl border px-3"
                            value={draft.provider.vendor}
                            onChange={(event) =>
                              setDraft((current) => {
                                const vendor = event.target.value as GatewayProviderBundle["provider"]["vendor"];
                                return {
                                  ...current,
                                  provider: {
                                    ...current.provider,
                                    vendor,
                                    auth_strategy: defaultAuthStrategyForVendor(vendor),
                                  },
                                };
                              })
                            }
                          >
                            <option value="openai">OpenAI</option>
                            <option value="anthropic">Anthropic</option>
                            <option value="gemini">Gemini</option>
                          </select>
                        </label>
                        <label className="grid gap-2 text-sm font-medium text-slate-600 dark:text-slate-300">
                          <span>启用状态</span>
                          <select
                            className="h-11 rounded-xl border px-3"
                            value={draft.provider.enabled ? "true" : "false"}
                            onChange={(event) =>
                              setDraft((current) => ({
                                ...current,
                                provider: {
                                  ...current.provider,
                                  enabled: event.target.value === "true",
                                },
                              }))
                            }
                          >
                            <option value="true">启用</option>
                            <option value="false">停用</option>
                          </select>
                        </label>
                        <div className="md:col-span-2">
                          <NyroTextField
                            label="Base URL"
                            value={draft.provider.base_url}
                            onChange={(event) =>
                              setDraft((current) => ({
                                ...current,
                                provider: { ...current.provider, base_url: event.target.value },
                              }))
                            }
                          />
                        </div>
                      </div>
                    </section>

                    <section className="rounded-[1.2rem] border border-white/70 bg-white/55 p-4 dark:border-white/10 dark:bg-white/5">
                      <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-50">鉴权策略</h4>
                      <div className="mt-4 grid gap-3 md:grid-cols-2">
                        <label className="grid gap-2 text-sm font-medium text-slate-600 dark:text-slate-300">
                          <span>鉴权方式</span>
                          <select
                            className="h-11 rounded-xl border px-3"
                            value={draft.provider.auth_strategy.kind}
                            onChange={(event) =>
                              setDraft((current) =>
                                setAuthKind(current, event.target.value as GatewayAuthStrategy["kind"]),
                              )
                            }
                          >
                            <option value="bearer">Bearer</option>
                            <option value="header_api_key">Header API Key</option>
                            <option value="query_api_key">Query API Key</option>
                          </select>
                        </label>

                        <NyroTextField
                          label={authParamLabel}
                          value={getAuthParam(draft.provider.auth_strategy)}
                          disabled={draft.provider.auth_strategy.kind === "bearer"}
                          onChange={(event) =>
                            setDraft((current) => {
                              const strategy = current.provider.auth_strategy;
                              if (strategy.kind === "header_api_key") {
                                return {
                                  ...current,
                                  provider: {
                                    ...current.provider,
                                    auth_strategy: {
                                      kind: "header_api_key",
                                      header_name: event.target.value,
                                    },
                                  },
                                };
                              }
                              if (strategy.kind === "query_api_key") {
                                return {
                                  ...current,
                                  provider: {
                                    ...current.provider,
                                    auth_strategy: {
                                      kind: "query_api_key",
                                      parameter_name: event.target.value,
                                    },
                                  },
                                };
                              }
                              return current;
                            })
                          }
                        />
                      </div>
                    </section>

                    <section className="rounded-[1.2rem] border border-white/70 bg-white/55 p-4 dark:border-white/10 dark:bg-white/5">
                      <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-50">每日重置时间</h4>
                      <div className="mt-4 grid gap-3 md:grid-cols-3">
                        <NyroTextField
                          label="时区"
                          value={draft.daily_reset.timezone}
                          onChange={(event) =>
                            setDraft((current) => ({
                              ...current,
                              daily_reset: { ...current.daily_reset, timezone: event.target.value },
                            }))
                          }
                        />
                        <NyroTextField
                          label="小时"
                          numbersOnly
                          value={toInputValue(draft.daily_reset.hour)}
                          onChange={(event) =>
                            setDraft((current) => ({
                              ...current,
                              daily_reset: {
                                ...current.daily_reset,
                                hour: Math.min(23, Number(event.target.value || "0")),
                              },
                            }))
                          }
                        />
                        <NyroTextField
                          label="分钟"
                          numbersOnly
                          value={toInputValue(draft.daily_reset.minute)}
                          onChange={(event) =>
                            setDraft((current) => ({
                              ...current,
                              daily_reset: {
                                ...current.daily_reset,
                                minute: Math.min(59, Number(event.target.value || "0")),
                              },
                            }))
                          }
                        />
                      </div>
                    </section>

                    <section className="rounded-[1.2rem] border border-white/70 bg-white/55 p-4 dark:border-white/10 dark:bg-white/5">
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div>
                          <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-50">Key 池</h4>
                          <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                            真实上游 API Key 应保存在这里，Nyro 到 gateway 的 hop 凭证不放在此处。
                          </p>
                        </div>
                        <div className="flex flex-wrap gap-2">
                          <Button variant="outline" onClick={addSingleKey}>
                            <KeyRound className="h-4 w-4" />
                            新增 Key
                          </Button>
                        </div>
                      </div>

                      <div className="mt-4 rounded-[1rem] border border-white/70 bg-white/70 p-4 dark:border-white/10 dark:bg-white/5">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <div>
                            <h5 className="text-sm font-semibold text-slate-900 dark:text-slate-50">批量导入</h5>
                            <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                              支持每行一个 Key。格式可为：`api_key`、`api_key,显示名`、`api_key,显示名,权重,启用状态`。
                              分隔符支持逗号、制表符或 `|`。
                            </p>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Button variant="outline" onClick={() => importBatchKeys("append")}>
                              <Upload className="h-4 w-4" />
                              追加导入
                            </Button>
                            <Button variant="danger" onClick={() => importBatchKeys("replace")}>
                              <Upload className="h-4 w-4" />
                              覆盖导入
                            </Button>
                          </div>
                        </div>
                        <div className="mt-3">
                          <NyroTextareaField
                            label="批量 Key 文本"
                            rows={6}
                            value={batchKeysText}
                            onChange={(event) => setBatchKeysText(event.target.value)}
                          />
                        </div>
                      </div>

                      <div className="mt-4 grid gap-4 xl:grid-cols-[340px_minmax(0,1fr)]">
                        <div className="rounded-[1rem] border border-white/70 bg-white/70 p-4 dark:border-white/10 dark:bg-white/5">
                          <div className="flex flex-wrap items-center justify-between gap-3">
                            <div>
                              <h5 className="text-sm font-semibold text-slate-900 dark:text-slate-50">紧凑列表</h5>
                              <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                                共 {draft.keys.length} 个 Key，已启用 {enabledKeyCount} 个。
                              </p>
                            </div>
                            <Badge variant="outline">滚动列表</Badge>
                          </div>
                          <div className="mt-3">
                            <NyroTextField
                              label="搜索 Key"
                              value={keySearchText}
                              onChange={(event) => setKeySearchText(event.target.value)}
                            />
                          </div>
                          <div className="mt-3 max-h-[32rem] space-y-2 overflow-y-auto pr-1">
                            {visibleKeyEntries.length === 0 ? (
                              <div className="rounded-xl border border-dashed border-slate-200 px-4 py-6 text-sm text-slate-500 dark:border-white/10 dark:text-slate-300">
                                没有匹配的 Key。
                              </div>
                            ) : (
                              visibleKeyEntries.map(({ key, index }) => (
                                <button
                                  key={`${index}-${key.id}-${key.api_key.slice(0, 8)}`}
                                  type="button"
                                  onClick={() => setSelectedKeyIndex(index)}
                                  className={cn(
                                    "w-full rounded-xl border px-3 py-3 text-left transition",
                                    selectedKeyIndex === index
                                      ? "border-slate-900 bg-slate-900 text-white"
                                      : "border-white/70 bg-white/80 text-slate-900 hover:bg-white dark:border-white/10 dark:bg-white/5 dark:text-slate-100 dark:hover:bg-white/10",
                                  )}
                                >
                                  <div className="flex items-start justify-between gap-3">
                                    <div className="min-w-0">
                                      <p className="truncate text-sm font-semibold">
                                        {key.display_name?.trim() || key.id || `Key ${index + 1}`}
                                      </p>
                                      <p className={cn("mt-1 truncate text-xs", selectedKeyIndex === index ? "text-white/70" : "text-slate-500 dark:text-slate-300")}>
                                        {key.id || "未设置 Key ID"}
                                      </p>
                                    </div>
                                    <Badge variant={key.enabled ? "success" : "warning"}>
                                      {key.enabled ? "启用" : "停用"}
                                    </Badge>
                                  </div>
                                  <div className={cn("mt-2 flex items-center justify-between gap-3 text-xs", selectedKeyIndex === index ? "text-white/70" : "text-slate-500 dark:text-slate-300")}>
                                    <span className="truncate">{key.api_key.slice(0, 12)}...</span>
                                    <span>权重 {key.weight ?? 1}</span>
                                  </div>
                                </button>
                              ))
                            )}
                          </div>
                        </div>

                        <div className="rounded-[1rem] border border-white/70 bg-white/70 p-4 dark:border-white/10 dark:bg-white/5">
                          {selectedKey ? (
                            <>
                              <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
                                <div>
                                  <h5 className="text-sm font-semibold text-slate-900 dark:text-slate-50">
                                    编辑当前 Key
                                  </h5>
                                  <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                                    当前选中：{selectedKey.display_name?.trim() || selectedKey.id || `Key ${selectedKeyIndex + 1}`}
                                  </p>
                                </div>
                                <Button
                                  variant="ghost"
                                  size="sm"
                                  onClick={() => removeKeyAt(selectedKeyIndex)}
                                >
                                  <Trash2 className="h-4 w-4" />
                                  删除当前 Key
                                </Button>
                              </div>
                              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                                <NyroTextField
                                  label="Key ID"
                                  value={selectedKey.id}
                                  onChange={(event) =>
                                    updateSelectedKey((key) => ({ ...key, id: event.target.value }))
                                  }
                                />
                                <NyroTextField
                                  label="显示名称"
                                  value={selectedKey.display_name ?? ""}
                                  onChange={(event) =>
                                    updateSelectedKey((key) => ({
                                      ...key,
                                      display_name: event.target.value,
                                    }))
                                  }
                                />
                                <NyroTextField
                                  label="权重"
                                  numbersOnly
                                  value={toInputValue(selectedKey.weight)}
                                  onChange={(event) =>
                                    updateSelectedKey((key) => ({
                                      ...key,
                                      weight: parseOptionalNumber(event.target.value),
                                    }))
                                  }
                                />
                                <label className="grid gap-2 text-sm font-medium text-slate-600 dark:text-slate-300">
                                  <span>状态</span>
                                  <select
                                    className="h-11 rounded-xl border px-3"
                                    value={selectedKey.enabled ? "true" : "false"}
                                    onChange={(event) =>
                                      updateSelectedKey((key) => ({
                                        ...key,
                                        enabled: event.target.value === "true",
                                      }))
                                    }
                                  >
                                    <option value="true">启用</option>
                                    <option value="false">停用</option>
                                  </select>
                                </label>
                                <div className="md:col-span-2 xl:col-span-4">
                                  <NyroTextareaField
                                    label="真实 API Key"
                                    rows={4}
                                    value={selectedKey.api_key}
                                    onChange={(event) =>
                                      updateSelectedKey((key) => ({
                                        ...key,
                                        api_key: event.target.value,
                                      }))
                                    }
                                  />
                                </div>
                              </div>
                            </>
                          ) : (
                            <div className="rounded-xl border border-dashed border-slate-200 px-4 py-8 text-sm text-slate-500 dark:border-white/10 dark:text-slate-300">
                              先从左侧选择一个 Key，或者先批量导入。
                            </div>
                          )}
                        </div>
                      </div>
                    </section>

                    <section className="rounded-[1.2rem] border border-white/70 bg-white/55 p-4 dark:border-white/10 dark:bg-white/5">
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div>
                          <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-50">模型限流规则</h4>
                          <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                            支持精确模型和 `*` 通配规则；若设置 TPM，gateway 会尝试进行输入 Token 估算。
                          </p>
                        </div>
                        <Button
                          variant="outline"
                          onClick={() =>
                            setDraft((current) => ({
                              ...current,
                              model_rules: [...current.model_rules, createRuleDraft(current.provider.id)],
                            }))
                          }
                        >
                          <CirclePlus className="h-4 w-4" />
                          新增规则
                        </Button>
                      </div>

                      <div className="mt-4 space-y-3">
                        {draft.model_rules.map((rule, index) => (
                          <div
                            key={`${index}-${rule.model}`}
                            className="rounded-[1rem] border border-white/70 bg-white/70 p-4 dark:border-white/10 dark:bg-white/5"
                          >
                            <div className="mb-3 flex items-center justify-between gap-3">
                              <Badge variant="outline">规则 {index + 1}</Badge>
                              <Button
                                variant="ghost"
                                size="sm"
                                onClick={() =>
                                  setDraft((current) => ({
                                    ...current,
                                    model_rules: current.model_rules.filter(
                                      (_, itemIndex) => itemIndex !== index,
                                    ),
                                  }))
                                }
                              >
                                <Trash2 className="h-4 w-4" />
                                删除
                              </Button>
                            </div>
                            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                              <NyroTextField
                                label="模型"
                                value={rule.model}
                                onChange={(event) =>
                                  setDraft((current) => ({
                                    ...current,
                                    model_rules: current.model_rules.map((item, itemIndex) =>
                                      itemIndex === index ? { ...item, model: event.target.value } : item,
                                    ),
                                  }))
                                }
                              />
                              <NyroTextField
                                label="RPM"
                                numbersOnly
                                value={toInputValue(rule.rpm)}
                                onChange={(event) =>
                                  setDraft((current) => ({
                                    ...current,
                                    model_rules: current.model_rules.map((item, itemIndex) =>
                                      itemIndex === index
                                        ? { ...item, rpm: parseOptionalNumber(event.target.value) }
                                        : item,
                                    ),
                                  }))
                                }
                              />
                              <NyroTextField
                                label="RPD"
                                numbersOnly
                                value={toInputValue(rule.rpd)}
                                onChange={(event) =>
                                  setDraft((current) => ({
                                    ...current,
                                    model_rules: current.model_rules.map((item, itemIndex) =>
                                      itemIndex === index
                                        ? { ...item, rpd: parseOptionalNumber(event.target.value) }
                                        : item,
                                    ),
                                  }))
                                }
                              />
                              <NyroTextField
                                label="TPM"
                                numbersOnly
                                value={toInputValue(rule.tpm)}
                                onChange={(event) =>
                                  setDraft((current) => ({
                                    ...current,
                                    model_rules: current.model_rules.map((item, itemIndex) =>
                                      itemIndex === index
                                        ? { ...item, tpm: parseOptionalNumber(event.target.value) }
                                        : item,
                                    ),
                                  }))
                                }
                              />
                              <label className="grid gap-2 text-sm font-medium text-slate-600 dark:text-slate-300">
                                <span>TPM 统计方式</span>
                                <select
                                  className="h-11 rounded-xl border px-3"
                                  value={rule.tpm_mode ?? ""}
                                  onChange={(event) =>
                                    setDraft((current) => ({
                                      ...current,
                                      model_rules: current.model_rules.map((item, itemIndex) =>
                                        itemIndex === index
                                          ? {
                                              ...item,
                                              tpm_mode: event.target.value
                                                ? (event.target.value as GatewayModelRule["tpm_mode"])
                                                : null,
                                            }
                                          : item,
                                      ),
                                    }))
                                  }
                                >
                                  <option value="">未设置</option>
                                  <option value="input_only">仅输入</option>
                                  <option value="input_and_output">输入 + 输出</option>
                                </select>
                              </label>
                              <label className="grid gap-2 text-sm font-medium text-slate-600 dark:text-slate-300">
                                <span>OpenAI Tokenizer</span>
                                <select
                                  className="h-11 rounded-xl border px-3"
                                  value={rule.tokenizer_encoding ?? ""}
                                  onChange={(event) =>
                                    setDraft((current) => ({
                                      ...current,
                                      model_rules: current.model_rules.map((item, itemIndex) =>
                                        itemIndex === index
                                          ? {
                                              ...item,
                                              tokenizer_encoding: event.target.value
                                                ? (event.target.value as GatewayModelRule["tokenizer_encoding"])
                                                : null,
                                            }
                                          : item,
                                      ),
                                    }))
                                  }
                                >
                                  <option value="">未设置</option>
                                  <option value="o200k_harmony">o200k_harmony</option>
                                  <option value="o200k_base">o200k_base</option>
                                  <option value="cl100k_base">cl100k_base</option>
                                </select>
                              </label>
                              <div className="md:col-span-2">
                                <NyroTextField
                                  label="Tokenizer 模型名"
                                  value={rule.tokenizer_model ?? ""}
                                  onChange={(event) =>
                                    setDraft((current) => ({
                                      ...current,
                                      model_rules: current.model_rules.map((item, itemIndex) =>
                                        itemIndex === index
                                          ? { ...item, tokenizer_model: event.target.value }
                                          : item,
                                      ),
                                    }))
                                  }
                                />
                              </div>
                            </div>
                          </div>
                        ))}
                      </div>
                    </section>

                    <section className="rounded-[1.2rem] border border-white/70 bg-white/55 p-4 dark:border-white/10 dark:bg-white/5">
                      <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-50">请求预览</h4>
                      <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                        这是保存时会发往 <code>PUT /admin/providers/:provider_id</code> 的完整 JSON。
                      </p>
                      <pre className="mt-4 overflow-auto rounded-[1rem] bg-slate-950 px-4 py-4 text-xs leading-6 text-slate-100">
                        {preview}
                      </pre>
                    </section>
                  </div>
                </section>
              </div>
            ) : (
              <div className="space-y-4">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <p className="text-sm leading-6 text-slate-500 dark:text-slate-300">
                    这里展示来自 limiter 的实时快照，包括每个模型的 RPM、RPD、TPM 占用情况，以及 Key 轮转可用性。
                  </p>
                  <Button onClick={() => void refreshRuntime()}>
                    <RefreshCw className="h-4 w-4" />
                    刷新运行态
                  </Button>
                </div>

                {runtimeBusy && (
                  <div className="flex items-center gap-2 rounded-[1.25rem] border border-dashed border-slate-200 px-4 py-6 text-sm text-slate-500">
                    <LoaderCircle className="h-4 w-4 animate-spin" />
                    正在读取运行态快照...
                  </div>
                )}

                {!runtimeBusy && runtimeViews.length === 0 && (
                  <div className="rounded-[1.25rem] border border-dashed border-slate-200 px-4 py-10 text-center text-sm text-slate-500">
                    还没有运行态数据。先发起请求，或者在 Provider 页点击“查看运行态”。
                  </div>
                )}

                {!runtimeBusy &&
                  runtimeViews.map((view) => (
                    <section key={view.summary.id} className="glass rounded-[1.25rem] p-4 md:p-5">
                      <div className="flex flex-wrap items-start justify-between gap-4">
                        <div>
                          <div className="flex flex-wrap items-center gap-2">
                            <h3 className="text-base font-semibold text-slate-900 dark:text-slate-50">
                              {view.summary.name || view.summary.id}
                            </h3>
                            <Badge variant={view.summary.enabled ? "success" : "warning"}>
                              {view.summary.enabled ? "启用" : "停用"}
                            </Badge>
                            <Badge variant="outline">{view.summary.vendor}</Badge>
                          </div>
                          <p className="mt-2 text-sm text-slate-500 dark:text-slate-300">
                            {view.summary.id} · {view.summary.base_url}
                          </p>
                        </div>
                        <div className="text-right text-xs text-slate-500 dark:text-slate-300">
                          <p>快照时间</p>
                          <p className="mt-1 text-sm font-medium text-slate-900 dark:text-slate-100">
                            {new Date(view.runtime.captured_at_ms).toLocaleString("zh-CN")}
                          </p>
                        </div>
                      </div>

                      <div className="mt-4 flex flex-wrap gap-2">
                        <Badge variant="outline">{view.summary.rate_limit.key_count} 个 Key</Badge>
                        <Badge variant="outline">{view.summary.rate_limit.model_rule_count} 条规则</Badge>
                        {view.summary.rate_limit.has_rpm && <Badge variant="secondary">启用 RPM</Badge>}
                        {view.summary.rate_limit.has_rpd && <Badge variant="secondary">启用 RPD</Badge>}
                        {view.summary.rate_limit.has_tpm && <Badge variant="secondary">启用 TPM</Badge>}
                      </div>

                      <div className="mt-4 grid gap-4 xl:grid-cols-2">
                        {view.runtime.models.map((model) => (
                          <article
                            key={`${view.summary.id}-${model.model}`}
                            className="rounded-[1.1rem] border border-white/70 bg-white/70 p-4 dark:border-white/10 dark:bg-white/5"
                          >
                            <div className="flex flex-wrap items-center justify-between gap-3">
                              <div>
                                <h4 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                                  {model.model}
                                </h4>
                                <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                                  命中规则：{model.matched_rule_model ?? "无"}
                                  {model.tpm_mode ? ` · TPM 模式：${model.tpm_mode}` : ""}
                                </p>
                              </div>
                              <Badge variant="outline">
                                可用 Key {model.available_key_count}/{model.enabled_key_count}
                              </Badge>
                            </div>

                            <div className="mt-4 grid gap-3 md:grid-cols-3">
                              <MetricCard label="RPM" used={model.keys.reduce((sum, item) => sum + item.rpm.used, 0)} limit={model.rpm_limit} />
                              <MetricCard label="RPD" used={model.keys.reduce((sum, item) => sum + item.rpd.used, 0)} limit={model.rpd_limit} />
                              <MetricCard label="TPM" used={model.keys.reduce((sum, item) => sum + item.tpm.used, 0)} limit={model.tpm_limit} />
                            </div>

                            <div className="mt-4 space-y-3">
                              {model.keys.map((key) => (
                                <div
                                  key={key.key_id}
                                  className="rounded-[1rem] border border-slate-200/80 bg-slate-50/80 p-3 dark:border-white/10 dark:bg-slate-950/40"
                                >
                                  <div className="flex flex-wrap items-center justify-between gap-3">
                                    <div>
                                      <p className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                                        {key.key_id}
                                      </p>
                                      <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
                                        活跃租约 {key.active_lease_count} · 日窗口 {key.rpd_window_id ?? "未开始"}
                                      </p>
                                    </div>
                                    <div className="flex flex-wrap gap-2">
                                      <Badge variant={key.enabled ? "success" : "warning"}>
                                        {key.enabled ? "启用" : "停用"}
                                      </Badge>
                                      <Badge variant={key.available ? "success" : "danger"}>
                                        {key.available ? "可用" : key.blocked_reason ?? "不可用"}
                                      </Badge>
                                    </div>
                                  </div>

                                  <div className="mt-3 grid gap-3 md:grid-cols-3">
                                    <MetricBar label="RPM" used={key.rpm.used} limit={key.rpm.limit} ratio={key.rpm.ratio} />
                                    <MetricBar label="RPD" used={key.rpd.used} limit={key.rpd.limit} ratio={key.rpd.ratio} />
                                    <MetricBar label="TPM" used={key.tpm.used} limit={key.tpm.limit} ratio={key.tpm.ratio} />
                                  </div>
                                </div>
                              ))}
                            </div>
                          </article>
                        ))}
                      </div>
                    </section>
                  ))}
              </div>
            )}
          </section>
        </main>
      </div>
    </div>
  );
}

function SidebarButton({
  active,
  icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-[13px] font-medium transition",
        active
          ? "bg-slate-900 text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.12),0_6px_14px_rgba(15,23,42,0.22)]"
          : "text-slate-600 hover:bg-white/70 hover:text-slate-900 dark:text-slate-200 dark:hover:bg-white/10 dark:hover:text-white",
      )}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

function MetricCard({
  label,
  used,
  limit,
}: {
  label: string;
  used: number;
  limit: number | null;
}) {
  const ratio = limit && limit > 0 ? used / limit : null;
  return (
    <div className="rounded-[1rem] border border-slate-200/80 bg-slate-50/85 p-3 dark:border-white/10 dark:bg-slate-950/40">
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs font-semibold tracking-[0.18em] text-slate-500 uppercase">{label}</span>
        <span className="text-xs text-slate-500 dark:text-slate-300">{formatPercent(ratio)}</span>
      </div>
      <p className="mt-2 text-lg font-semibold text-slate-900 dark:text-slate-100">
        {formatNumber(used)}
      </p>
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-300">
        限额 {formatNumber(limit)}
      </p>
    </div>
  );
}

function MetricBar({
  label,
  used,
  limit,
  ratio,
}: {
  label: string;
  used: number;
  limit: number | null;
  ratio: number | null;
}) {
  const clamped = Math.max(0, Math.min(100, Math.round((ratio ?? 0) * 100)));
  const colorClass =
    ratio === null ? "bg-slate-300" : ratio >= 0.9 ? "bg-red-500" : ratio >= 0.6 ? "bg-amber-500" : "bg-emerald-500";

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3 text-xs text-slate-500 dark:text-slate-300">
        <span className="font-medium text-slate-700 dark:text-slate-100">{label}</span>
        <span>
          {formatNumber(used)} / {formatNumber(limit)}
        </span>
      </div>
      <div className="h-2 rounded-full bg-slate-200 dark:bg-slate-800">
        <div
          className={cn("h-2 rounded-full transition-all", colorClass)}
          style={{ width: `${ratio === null ? 0 : clamped}%` }}
        />
      </div>
      <p className="text-[11px] text-slate-500 dark:text-slate-300">{formatPercent(ratio)}</p>
    </div>
  );
}

function readErrorMessage(error: unknown) {
  if (error instanceof ApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "发生未知错误。";
}
