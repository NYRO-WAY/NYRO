import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatNumber(value: number | null | undefined) {
  if (value === null || value === undefined) {
    return "未设置";
  }
  return new Intl.NumberFormat("zh-CN").format(value);
}

export function formatPercent(ratio: number | null | undefined) {
  if (ratio === null || ratio === undefined) {
    return "未设置";
  }
  return `${Math.round(ratio * 100)}%`;
}

export function maskSecret(value: string) {
  const trimmed = value.trim();
  if (trimmed.length <= 8) {
    return "********";
  }
  return `${trimmed.slice(0, 4)}***${trimmed.slice(-4)}`;
}

export function toInputValue(value: number | null | undefined) {
  return value === null || value === undefined ? "" : String(value);
}

export function parseOptionalNumber(raw: string) {
  const trimmed = raw.trim();
  if (!trimmed) {
    return null;
  }
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : null;
}

export function buildRouteHint(vendor: "openai" | "anthropic" | "gemini", providerId: string) {
  const id = providerId.trim() || "<provider-id>";
  if (vendor === "openai") {
    return `/providers/${id}/openai/v1/chat/completions`;
  }
  if (vendor === "anthropic") {
    return `/providers/${id}/anthropic/v1/messages`;
  }
  return `/providers/${id}/google/v1beta/models/gemini-2.5-pro:generateContent`;
}
