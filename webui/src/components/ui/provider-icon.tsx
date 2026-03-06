import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";

const iconModules = import.meta.glob("../../assets/icons/*.svg", {
  import: "default",
}) as Record<string, () => Promise<string>>;

const iconLoaderMap: Record<string, () => Promise<string>> = {};
for (const [path, loader] of Object.entries(iconModules)) {
  const matched = path.match(/\/([^/]+)\.svg$/);
  if (matched?.[1]) {
    iconLoaderMap[matched[1].toLowerCase()] = loader;
  }
}
const iconUrlCache = new Map<string, string>();

interface ProviderIconProps {
  name?: string;
  protocol?: string;
  baseUrl?: string;
  size?: number;
  className?: string;
}

const ICON_ALIASES: Record<string, string> = {
  claude: "anthropic",
  chatgpt: "openai",
  gpt: "openai",
  googleai: "google",
  googleapis: "google",
  generativelanguage: "gemini",
  tongyi: "qwen",
  dashscope: "qwen",
  modelscope: "modelscope-color",
  aihubmix: "aihubmix-color",
  longcat: "longcat-color",
  moonshot: "kimi",
  hunyuan: "tencent",
  glm: "zhipu",
  chatglm: "zhipu",
};

function tokenize(value?: string): string[] {
  if (!value) return [];
  return value
    .toLowerCase()
    .split(/[^a-z0-9]+/g)
    .map((s) => s.trim())
    .filter(Boolean);
}

function hostTokens(baseUrl?: string): string[] {
  if (!baseUrl) return [];
  try {
    return tokenize(new URL(baseUrl).hostname);
  } catch {
    return tokenize(baseUrl);
  }
}

function normalizeToken(token: string): string {
  return ICON_ALIASES[token] ?? token;
}

export function resolveProviderIconKey({
  name,
  protocol,
  baseUrl,
}: {
  name?: string;
  protocol?: string;
  baseUrl?: string;
}): string | null {
  const raw = [...tokenize(protocol), ...tokenize(name), ...hostTokens(baseUrl)];
  const candidates = raw.flatMap((token) => {
    const normalized = normalizeToken(token);
    return normalized === token ? [token] : [normalized, token];
  });

  for (const key of candidates) {
    if (key in iconLoaderMap) return key;
  }
  return null;
}

export function ProviderIcon({
  name,
  protocol,
  baseUrl,
  size = 20,
  className,
}: ProviderIconProps) {
  const iconKey = resolveProviderIconKey({ name, protocol, baseUrl });
  const [iconUrl, setIconUrl] = useState<string>("");
  const fallback = (name || protocol || "?").slice(0, 1).toUpperCase();

  useEffect(() => {
    if (!iconKey) {
      setIconUrl("");
      return;
    }
    const cached = iconUrlCache.get(iconKey);
    if (cached) {
      setIconUrl(cached);
      return;
    }
    const loader = iconLoaderMap[iconKey];
    if (!loader) {
      setIconUrl("");
      return;
    }
    let cancelled = false;
    loader()
      .then((url) => {
        if (cancelled) return;
        iconUrlCache.set(iconKey, url);
        setIconUrl(url);
      })
      .catch(() => {
        if (!cancelled) setIconUrl("");
      });

    return () => {
      cancelled = true;
    };
  }, [iconKey]);

  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center overflow-hidden rounded-md border border-slate-200 bg-white/85 text-[10px] font-semibold text-slate-500",
        className,
      )}
      style={{ width: size, height: size }}
      title={name || protocol || "provider"}
    >
      {iconUrl ? (
        <img
          src={iconUrl}
          alt=""
          aria-hidden="true"
          className="h-[78%] w-[78%] object-contain"
        />
      ) : (
        fallback
      )}
    </span>
  );
}
