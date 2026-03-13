import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { backend } from "@/lib/backend";
import type { Provider, CreateProvider, UpdateProvider, TestResult } from "@/lib/types";
import {
  Server,
  Plus,
  Trash2,
  CheckCircle,
  XCircle,
  Zap,
  Loader2,
  Pencil,
  X,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { useLocale } from "@/lib/i18n";
import { ProviderIcon } from "@/components/ui/provider-icon";
import { NyroIcon } from "@/components/ui/nyro-icon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

type ProviderProtocol = "openai" | "anthropic" | "gemini";

type ProviderChannelPreset = {
  id: string;
  label: {
    zh: string;
    en: string;
  };
  baseUrls: Partial<Record<ProviderProtocol, string>>;
  modelsEndpoint?: string;
  staticModels?: string[];
};

type ProviderPreset = {
  id: string;
  label: {
    zh: string;
    en: string;
  };
  iconName?: string;
  defaultProtocol: ProviderProtocol;
  channels?: ProviderChannelPreset[];
};

function protocolUrl(protocol: string) {
  switch (protocol) {
    case "anthropic": return "https://api.anthropic.com";
    case "gemini": return "https://generativelanguage.googleapis.com";
    default: return "https://api.openai.com";
  }
}

const emptyCreate: CreateProvider = {
  name: "",
  protocol: "openai",
  base_url: "https://api.openai.com",
  preset_key: "",
  channel: "",
  models_endpoint: "",
  static_models: "",
  api_key: "",
};
const PAGE_SIZE = 6;
const DEFAULT_PRESET_ID = "custom";
const protocolOptions = [
  { label: "OpenAI", value: "openai" },
  { label: "Anthropic", value: "anthropic" },
  { label: "Gemini", value: "gemini" },
];
const providerPresets: ProviderPreset[] = [
  {
    id: "custom",
    label: { zh: "自定义", en: "Custom" },
    defaultProtocol: "openai",
    channels: [],
  },
  {
    id: "openai",
    label: { zh: "OpenAI", en: "OpenAI" },
    iconName: "openai",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: { openai: "https://api.openai.com/v1" },
      },
    ],
  },
  {
    id: "anthropic",
    label: { zh: "Anthropic", en: "Anthropic" },
    iconName: "anthropic",
    defaultProtocol: "anthropic",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          anthropic: "https://api.anthropic.com",
        },
      },
    ],
  },
  {
    id: "google",
    label: { zh: "Google", en: "Google" },
    iconName: "google",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://generativelanguage.googleapis.com/v1beta/openai",
          gemini: "https://generativelanguage.googleapis.com",
        },
      },
    ],
  },
  {
    id: "xai",
    label: { zh: "xAI", en: "xAI" },
    iconName: "xai",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://api.x.ai/v1",
        },
      },
    ],
  },
  {
    id: "deepseek",
    label: { zh: "DeepSeek", en: "DeepSeek" },
    iconName: "deepseek",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://api.deepseek.com/v1",
          anthropic: "https://api.deepseek.com/anthropic",
        },
      },
    ],
  },
  {
    id: "kimi",
    label: { zh: "Kimi", en: "Kimi" },
    iconName: "kimi",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://api.moonshot.ai/v1",
          anthropic: "https://api.moonshot.ai/anthropic",
        },
      },
      {
        id: "china",
        label: { zh: "中国站", en: "China" },
        baseUrls: {
          openai: "https://api.moonshot.cn/v1",
          anthropic: "https://api.moonshot.cn/anthropic",
        },
      },
    ],
  },
  {
    id: "minimax",
    label: { zh: "MiniMax", en: "MiniMax" },
    iconName: "minimax",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://api.minimax.io/v1",
          anthropic: "https://api.minimax.io/anthropic",
        },
        modelsEndpoint: "",
        staticModels: [],
      },
      {
        id: "china",
        label: { zh: "中国站", en: "China" },
        baseUrls: {
          openai: "https://api.minimaxi.com/v1",
          anthropic: "https://api.minimaxi.com/anthropic",
        },
        modelsEndpoint: "",
        staticModels: [],
      },
    ],
  },
  {
    id: "zhipu",
    label: { zh: "Zhipu", en: "Zhipu" },
    iconName: "zhipu",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://api.z.ai/api/paas/v4",
          anthropic: "https://api.z.ai/api/anthropic",
        },
      },
      {
        id: "china",
        label: { zh: "中国站", en: "China" },
        baseUrls: {
          openai: "https://open.bigmodel.cn/api/paas/v4",
          anthropic: "https://open.bigmodel.cn/api/anthropic",
        },
      },
    ],
  },
  {
    id: "nvidia",
    label: { zh: "NVIDIA", en: "NVIDIA" },
    iconName: "nvidia",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://integrate.api.nvidia.com/v1",
        },
      },
    ],
  },
  {
    id: "openrouter",
    label: { zh: "OpenRouter", en: "OpenRouter" },
    iconName: "openrouter",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "https://openrouter.ai/api/v1",
          anthropic: "https://openrouter.ai/api",
        },
      },
    ],
  },
  {
    id: "ollama",
    label: { zh: "Ollama", en: "Ollama" },
    iconName: "ollama",
    defaultProtocol: "openai",
    channels: [
      {
        id: "default",
        label: { zh: "默认", en: "Default" },
        baseUrls: {
          openai: "http://127.0.0.1:11434/v1",
        },
      },
    ],
  },
];

function presetLabel(preset: ProviderPreset, isZh: boolean) {
  return isZh ? preset.label.zh : preset.label.en;
}

function channelLabel(channel: ProviderChannelPreset, isZh: boolean) {
  return isZh ? channel.label.zh : channel.label.en;
}

function toGatewayBaseUrl(url: string, protocol: ProviderProtocol) {
  const normalized = url.trim().replace(/\/+$/, "");
  if (protocol === "openai") {
    return normalized.replace(/\/v1$/, "");
  }
  return normalized;
}

function defaultModelsEndpoint(baseUrl: string, protocol: ProviderProtocol) {
  const normalized = baseUrl.trim().replace(/\/+$/, "");

  if (protocol === "openai" || protocol === "anthropic") {
    try {
      const pathname = new URL(normalized).pathname.replace(/\/+$/, "");
      return pathname && pathname !== "/" ? `${normalized}/models` : `${normalized}/v1/models`;
    } catch {
      return normalized.endsWith("/v1") ? `${normalized}/models` : `${normalized}/v1/models`;
    }
  }

  if (protocol === "gemini") {
    return `${normalized}/v1beta/models`;
  }

  return "";
}

function joinStaticModels(models?: string[]) {
  return models?.join("\n") ?? "";
}

function fallbackChannelPreset(): ProviderChannelPreset {
  return {
    id: "default",
    label: { zh: "默认", en: "Default" },
    baseUrls: {},
  };
}

function presetChannels(preset?: ProviderPreset | null) {
  return preset?.channels?.length ? preset.channels : [fallbackChannelPreset()];
}

function resolvePresetConfig(
  preset: ProviderPreset,
  protocol: ProviderProtocol,
  channelId?: string,
) {
  const channel = presetChannels(preset).find((item) => item.id === channelId) ?? presetChannels(preset)[0];
  const sourceBaseUrls = channel?.baseUrls ?? {};
  const rawBaseUrl = sourceBaseUrls[protocol] ?? protocolUrl(protocol);
  const baseUrl = rawBaseUrl ? toGatewayBaseUrl(rawBaseUrl, protocol) : protocolUrl(protocol);
  const modelsEndpoint = channel?.modelsEndpoint ?? defaultModelsEndpoint(baseUrl, protocol);
  const staticModels = joinStaticModels(channel?.staticModels);

  return {
    baseUrl,
    modelsEndpoint,
    staticModels,
    channel,
  };
}

function FieldLabel({ children }: { children: string }) {
  return <label className="ml-1 text-xs leading-none font-normal text-slate-900">{children}</label>;
}

export default function ProvidersPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";

  const qc = useQueryClient();
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<Record<string, TestResult>>({});
  const [selectedPresetId, setSelectedPresetId] = useState(DEFAULT_PRESET_ID);

  const { data: providers = [], isLoading } = useQuery<Provider[]>({
    queryKey: ["providers"],
    queryFn: () => backend("get_providers"),
  });

  const [form, setForm] = useState<CreateProvider>(emptyCreate);
  const selectedPreset = useMemo(
    () => providerPresets.find((preset) => preset.id === selectedPresetId) ?? null,
    [selectedPresetId],
  );
  const [editForm, setEditForm] = useState<UpdateProvider & { id: string }>({
    id: "",
    name: "",
    protocol: "",
    base_url: "",
    preset_key: "",
    channel: "",
    models_endpoint: "",
    static_models: "",
    api_key: "",
  });

  const createMut = useMutation({
    mutationFn: (input: CreateProvider) => backend("create_provider", { input }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["providers"] });
      setShowForm(false);
      setSelectedPresetId(DEFAULT_PRESET_ID);
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
      preset_key: p.preset_key || DEFAULT_PRESET_ID,
      channel: p.channel || "default",
      models_endpoint: p.models_endpoint ?? "",
      static_models: p.static_models ?? "",
      api_key: "",
    });
  }

  function handlePresetChange(nextPresetId: string) {
    if (!nextPresetId) return;
    setSelectedPresetId(nextPresetId);
    const preset = providerPresets.find((item) => item.id === nextPresetId);
    if (!preset) return;

    if (preset.id === "custom") {
      setForm({
        ...emptyCreate,
        name: "",
        protocol: "openai",
        base_url: "",
        preset_key: DEFAULT_PRESET_ID,
        channel: "default",
      });
      return;
    }

    const nextChannelId = preset.channels?.[0]?.id ?? "";
    const config = resolvePresetConfig(preset, preset.defaultProtocol, nextChannelId);

    setForm((prev) => ({
      ...prev,
      name: preset.label.en,
      protocol: preset.defaultProtocol,
      base_url: config.baseUrl,
      preset_key: preset.id,
      channel: nextChannelId,
      models_endpoint: config.modelsEndpoint,
      static_models: config.staticModels,
    }));
  }

  function handlePresetChannelChange(nextChannelId: string) {
    if (!selectedPreset) return;
    const nextProtocol = form.protocol as ProviderProtocol;
    const config = resolvePresetConfig(selectedPreset, nextProtocol, nextChannelId);
    setForm((prev) => ({
      ...prev,
      channel: nextChannelId,
      base_url: config.baseUrl,
      models_endpoint: config.modelsEndpoint,
      static_models: config.staticModels,
    }));
  }

  function handleEditPresetChange(nextPresetId: string) {
    if (!nextPresetId) return;
    const preset = providerPresets.find((item) => item.id === nextPresetId);
    if (!preset) return;

    if (preset.id === DEFAULT_PRESET_ID) {
      setEditForm((prev) => (prev ? { ...prev, preset_key: DEFAULT_PRESET_ID, channel: "default" } : prev));
      return;
    }

    const nextChannelId = preset.channels?.[0]?.id ?? "";
    setEditForm((prev) =>
      prev
        ? (() => {
            const nextProtocol = (prev.protocol as ProviderProtocol) || preset.defaultProtocol;
            const config = resolvePresetConfig(preset, nextProtocol, nextChannelId);
            return {
              ...prev,
              preset_key: preset.id,
              channel: nextChannelId,
              protocol: nextProtocol,
              base_url: config.baseUrl,
              models_endpoint: config.modelsEndpoint,
              static_models: config.staticModels,
            };
          })()
        : prev,
    );
  }

  function closeCreateForm() {
    setShowForm(false);
    setSelectedPresetId(DEFAULT_PRESET_ID);
    setForm(emptyCreate);
  }

  const totalPages = Math.max(1, Math.ceil(providers.length / PAGE_SIZE));
  const pagedProviders = providers.slice(page * PAGE_SIZE, page * PAGE_SIZE + PAGE_SIZE);
  const createChannelOptions = selectedPreset ? presetChannels(selectedPreset) : [fallbackChannelPreset()];
  const createChannelValue =
    selectedPreset?.channels?.length
      ? (form.channel || createChannelOptions[0]?.id || "")
      : (createChannelOptions[0]?.id ?? "default");

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
        <Button
          onClick={() => {
            setEditingId(null);
            if (showForm) {
              closeCreateForm();
              return;
            }
            setShowForm(true);
            handlePresetChange(DEFAULT_PRESET_ID);
          }}
          className="flex items-center gap-2"
        >
          <Plus className="h-4 w-4" />
          {isZh ? "新增提供商" : "Add Provider"}
        </Button>
      </div>

      {/* Create Form */}
      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-6">
          <h2 className="text-lg font-semibold text-slate-900">{isZh ? "新建提供商" : "New Provider"}</h2>
          <div className="space-y-3">
            <div>
              <p className="text-sm font-semibold text-slate-700">
                {isZh ? "1. 快速选择常用模型供应商（可选）" : "1. Quick Select A Common Provider (Optional)"}
              </p>
              <p className="mt-1 text-xs text-slate-500">
                {isZh
                  ? "选择后会自动填充协议与 Base URL，后续仍可继续修改。"
                  : "Selecting a preset will prefill protocol and base URL, and you can still edit them."}
              </p>
            </div>
            <ToggleGroup
              type="single"
              value={selectedPresetId}
              onValueChange={handlePresetChange}
              className="provider-preset-group"
            >
              {[...providerPresets]
                .sort((a, b) => (a.id === DEFAULT_PRESET_ID ? -1 : b.id === DEFAULT_PRESET_ID ? 1 : 0))
                .map((preset) => (
                <ToggleGroupItem
                  key={preset.id}
                  value={preset.id}
                  variant="outline"
                  size="lg"
                  className="provider-preset-card h-auto w-full flex-col gap-3 px-4 py-5"
                  aria-label={presetLabel(preset, isZh)}
                >
                  {preset.id === "custom" ? (
                    <>
                      <NyroIcon
                        size={26}
                        className="provider-preset-icon provider-preset-icon-custom provider-preset-icon-colored"
                      />
                      <NyroIcon
                        size={26}
                        monochrome
                        className="provider-preset-icon provider-preset-icon-custom provider-preset-icon-mono"
                      />
                    </>
                  ) : (
                    <>
                      <ProviderIcon
                        name={preset.iconName ?? preset.label.en}
                        size={26}
                        className="provider-preset-icon provider-preset-icon-colored rounded-none border-0 bg-transparent"
                      />
                      <ProviderIcon
                        name={preset.iconName ?? preset.label.en}
                        size={26}
                        monochrome
                        className="provider-preset-icon provider-preset-icon-mono rounded-none border-0 bg-transparent"
                      />
                    </>
                  )}
                  <span className="provider-preset-label">{presetLabel(preset, isZh)}</span>
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
          <div className="h-px bg-slate-200/70" />
          <div className="space-y-4">
            <div>
              <p className="text-sm font-semibold text-slate-700">
                {isZh ? "2. 基础信息" : "2. Basic Information"}
              </p>
              <p className="mt-1 text-xs text-slate-500">
                {selectedPreset
                  ? (isZh
                    ? `已套用 ${presetLabel(selectedPreset, true)} 预设，可继续修改。`
                    : `${presetLabel(selectedPreset, false)} preset applied. You can continue editing.`)
                  : (isZh
                    ? "也可以跳过第一步，直接手动填写。"
                    : "You can also skip step one and fill everything manually.")}
              </p>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="col-span-2 space-y-2">
                <ToggleGroup
                  type="single"
                  value={createChannelValue}
                  onValueChange={(value) => {
                    if (!value || !selectedPreset?.channels?.length) return;
                    handlePresetChannelChange(value);
                  }}
                  className="provider-channel-group"
                >
                  {createChannelOptions.map((channel) => (
                    <ToggleGroupItem
                      key={channel.id}
                      value={channel.id}
                      variant="outline"
                      size="default"
                      className="provider-preset-card provider-channel-item"
                    >
                      {channelLabel(channel, isZh)}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>
              <div className="space-y-2">
                <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
                <Input
                  placeholder={isZh ? "例如 OpenAI 生产" : "e.g. OpenAI Production"}
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </div>
              <div className="space-y-2">
                <FieldLabel>{isZh ? "协议" : "Protocol"}</FieldLabel>
                <Select
                  value={form.protocol}
                  onValueChange={(value) => {
                    const nextProtocol = value as ProviderProtocol;
                    const config = selectedPreset
                      ? resolvePresetConfig(selectedPreset, nextProtocol, form.channel)
                      : {
                          baseUrl: protocolUrl(nextProtocol),
                          modelsEndpoint: defaultModelsEndpoint(protocolUrl(nextProtocol), nextProtocol),
                          staticModels: form.static_models ?? "",
                        };
                    setForm({
                      ...form,
                      protocol: nextProtocol,
                      base_url: config.baseUrl,
                      models_endpoint: config.modelsEndpoint,
                      static_models: config.staticModels,
                    });
                  }}
                >
                  <SelectTrigger>
                    <SelectValue placeholder={isZh ? "选择协议" : "Select protocol"} />
                  </SelectTrigger>
                  <SelectContent>
                    {protocolOptions.map((option) => (
                      <SelectItem key={option.value} value={option.value}>
                        <span className="flex items-center gap-2">
                          <ProviderIcon protocol={option.value} size={16} monochrome />
                          <span>{option.label}</span>
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <FieldLabel>{isZh ? "Base URL" : "Base URL"}</FieldLabel>
                <Input
                  placeholder={isZh ? "输入上游基础地址" : "Enter upstream base URL"}
                  value={form.base_url}
                  onChange={(e) => setForm({ ...form, base_url: e.target.value })}
                />
              </div>
              <div className="space-y-2">
                <FieldLabel>{isZh ? "Model Discovery" : "Model Discovery"}</FieldLabel>
                <Input
                  placeholder={isZh ? "可选，用于自动获取模型列表" : "Optional, used to auto-discover models"}
                  value={form.models_endpoint ?? ""}
                  onChange={(e) => setForm({ ...form, models_endpoint: e.target.value })}
                />
              </div>
              <div className="col-span-2 space-y-2">
                <FieldLabel>API Key</FieldLabel>
                <Input
                  placeholder="sk-..."
                  type="password"
                  value={form.api_key}
                  onChange={(e) => setForm({ ...form, api_key: e.target.value })}
                />
              </div>
            </div>
            <div className="flex gap-3">
              <Button
                onClick={() => createMut.mutate(form)}
                disabled={createMut.isPending || !form.name || !form.api_key}
              >
                {createMut.isPending ? (isZh ? "创建中..." : "Creating...") : (isZh ? "创建" : "Create")}
              </Button>
              <Button
                onClick={closeCreateForm}
                variant="secondary"
              >
                {isZh ? "取消" : "Cancel"}
              </Button>
            </div>
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
            const editingPresetId = editForm.preset_key || DEFAULT_PRESET_ID;
            const editingPreset =
              providerPresets.find((preset) => preset.id === editingPresetId) ?? providerPresets[0] ?? null;

            if (isEditing) {
              const editingChannelOptions = presetChannels(editingPreset);
              const editingChannelValue =
                editingPreset?.channels?.length
                  ? (editForm.channel || editingChannelOptions[0]?.id || "")
                  : (editingChannelOptions[0]?.id ?? "default");
              return (
                <div key={p.id} className="glass rounded-2xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-900">{isZh ? "编辑提供商" : "Edit Provider"}</h3>
                    <button onClick={() => setEditingId(null)} className="p-1 text-slate-400 hover:text-slate-600 cursor-pointer">
                      <X className="h-4 w-4" />
                    </button>
                  </div>
                  <div className="space-y-3">
                    <p className="text-sm font-semibold text-slate-700">
                      {isZh ? "1. 供应商" : "1. Provider"}
                    </p>
                    <ToggleGroup
                      type="single"
                      value={editingPresetId}
                      onValueChange={handleEditPresetChange}
                      className="provider-preset-group"
                    >
                      {[...providerPresets]
                        .sort((a, b) => (a.id === DEFAULT_PRESET_ID ? -1 : b.id === DEFAULT_PRESET_ID ? 1 : 0))
                        .map((preset) => (
                        <ToggleGroupItem
                          key={preset.id}
                          value={preset.id}
                          variant="outline"
                          size="lg"
                          className="provider-preset-card h-auto w-full flex-col gap-3 px-4 py-5"
                          aria-label={presetLabel(preset, isZh)}
                        >
                          {preset.id === "custom" ? (
                            <>
                              <NyroIcon
                                size={26}
                                className="provider-preset-icon provider-preset-icon-custom provider-preset-icon-colored"
                              />
                              <NyroIcon
                                size={26}
                                monochrome
                                className="provider-preset-icon provider-preset-icon-custom provider-preset-icon-mono"
                              />
                            </>
                          ) : (
                            <>
                              <ProviderIcon
                                name={preset.iconName ?? preset.label.en}
                                size={26}
                                className="provider-preset-icon provider-preset-icon-colored rounded-none border-0 bg-transparent"
                              />
                              <ProviderIcon
                                name={preset.iconName ?? preset.label.en}
                                size={26}
                                monochrome
                                className="provider-preset-icon provider-preset-icon-mono rounded-none border-0 bg-transparent"
                              />
                            </>
                          )}
                          <span className="provider-preset-label">{presetLabel(preset, isZh)}</span>
                        </ToggleGroupItem>
                      ))}
                    </ToggleGroup>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div className="col-span-2 space-y-2">
                      <FieldLabel>{isZh ? "渠道" : "Channel"}</FieldLabel>
                      <ToggleGroup
                        type="single"
                        value={editingChannelValue}
                        onValueChange={(value) => {
                          if (!value || !editingPreset?.channels?.length) return;
                          const config = resolvePresetConfig(
                            editingPreset,
                            (editForm.protocol as ProviderProtocol) || editingPreset.defaultProtocol,
                            value,
                          );
                          setEditForm({
                            ...editForm,
                            channel: value,
                            base_url: config.baseUrl,
                            models_endpoint: config.modelsEndpoint,
                            static_models: config.staticModels,
                          });
                        }}
                        className="provider-channel-group"
                      >
                        {editingChannelOptions.map((channel) => (
                          <ToggleGroupItem
                            key={channel.id}
                            value={channel.id}
                            variant="outline"
                            size="default"
                            className="provider-preset-card provider-channel-item"
                          >
                            {channelLabel(channel, isZh)}
                          </ToggleGroupItem>
                        ))}
                      </ToggleGroup>
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "名称" : "Name"}</FieldLabel>
                      <Input
                        placeholder={isZh ? "提供商名称" : "Provider name"}
                        value={editForm.name ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "协议" : "Protocol"}</FieldLabel>
                      <Select
                        value={editForm.protocol ?? ""}
                        onValueChange={(value) => {
                          const nextProtocol = value as ProviderProtocol;
                          const config = editingPreset
                            ? resolvePresetConfig(editingPreset, nextProtocol, editForm.channel ?? undefined)
                            : {
                                baseUrl: protocolUrl(nextProtocol),
                                modelsEndpoint: defaultModelsEndpoint(protocolUrl(nextProtocol), nextProtocol),
                                staticModels: editForm.static_models ?? "",
                              };
                          setEditForm({
                            ...editForm,
                            protocol: nextProtocol,
                            base_url: config.baseUrl,
                            models_endpoint: config.modelsEndpoint,
                            static_models: config.staticModels,
                          });
                        }}
                      >
                        <SelectTrigger>
                          <SelectValue placeholder={isZh ? "选择协议" : "Select protocol"} />
                        </SelectTrigger>
                        <SelectContent>
                          {protocolOptions.map((option) => (
                            <SelectItem key={option.value} value={option.value}>
                              <span className="flex items-center gap-2">
                                <ProviderIcon protocol={option.value} size={16} monochrome />
                                <span>{option.label}</span>
                              </span>
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "Base URL" : "Base URL"}</FieldLabel>
                      <Input
                        placeholder={isZh ? "输入上游基础地址" : "Enter upstream base URL"}
                        value={editForm.base_url ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, base_url: e.target.value })}
                      />
                    </div>
                    <div className="space-y-2">
                      <FieldLabel>{isZh ? "Model Discovery" : "Model Discovery"}</FieldLabel>
                      <Input
                        placeholder={isZh ? "可选，用于自动获取模型列表" : "Optional, used to auto-discover models"}
                        value={editForm.models_endpoint ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, models_endpoint: e.target.value })}
                      />
                    </div>
                    <div className="col-span-2 space-y-2">
                      <FieldLabel>{isZh ? "API Key" : "API Key"}</FieldLabel>
                      <Input
                        placeholder={isZh ? "留空则保持不变" : "Leave empty to keep current"}
                        type="password"
                        value={editForm.api_key ?? ""}
                        onChange={(e) => setEditForm({ ...editForm, api_key: e.target.value })}
                      />
                    </div>
                  </div>
                  <div className="flex gap-3">
                    <Button
                      onClick={() => {
                        setEditError(null);
                        const input: UpdateProvider = {
                          name: editForm.name || undefined,
                          protocol: editForm.protocol || undefined,
                          base_url: editForm.base_url || undefined,
                          preset_key: editForm.preset_key || undefined,
                          channel: editForm.channel || undefined,
                          models_endpoint: editForm.models_endpoint || undefined,
                          static_models: editForm.static_models || undefined,
                          api_key: editForm.api_key || undefined,
                        };
                        updateMut.mutate({ id: editForm.id, ...input });
                      }}
                      disabled={updateMut.isPending}
                    >
                      {updateMut.isPending ? (isZh ? "保存中..." : "Saving...") : (isZh ? "保存" : "Save")}
                    </Button>
                    <Button
                      onClick={() => { setEditingId(null); setEditError(null); }}
                      variant="secondary"
                    >
                      {isZh ? "取消" : "Cancel"}
                    </Button>
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
                        size={40}
                        fill
                        className="provider-preset-icon provider-preset-icon-colored rounded-xl border border-slate-300/70 bg-transparent"
                      />
                      <ProviderIcon
                        name={p.name}
                        protocol={p.protocol}
                        baseUrl={p.base_url}
                        size={40}
                        fill
                        monochrome
                        className="provider-preset-icon provider-preset-icon-mono rounded-xl border border-slate-300/70 bg-transparent"
                      />
                    </div>
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-slate-900">{p.name}</span>
                        <span className="protocol-pill inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium uppercase">
                          <ProviderIcon
                            protocol={p.protocol}
                            size={12}
                            className="provider-preset-icon provider-preset-icon-colored rounded-sm border-0 bg-transparent"
                          />
                          <ProviderIcon
                            protocol={p.protocol}
                            size={12}
                            monochrome
                            className="provider-preset-icon provider-preset-icon-mono rounded-sm border-0 bg-transparent"
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
