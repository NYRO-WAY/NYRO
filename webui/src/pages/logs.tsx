import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { backend } from "@/lib/backend";
import type { LogPage, LogQuery, Provider } from "@/lib/types";
import { ScrollText, ChevronLeft, ChevronRight } from "lucide-react";
import { useLocale } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const PAGE_SIZE = 12;
const ALL_OPTION = "__all__";

export default function LogsPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";

  const [page, setPage] = useState(0);
  const [filter, setFilter] = useState<LogQuery>({ limit: PAGE_SIZE, offset: 0 });

  const query: LogQuery = { ...filter, limit: PAGE_SIZE, offset: page * PAGE_SIZE };

  const { data, isLoading } = useQuery<LogPage>({
    queryKey: ["logs", query],
    queryFn: () => backend("query_logs", { query }),
    refetchInterval: 5_000,
  });
  const { data: providers = [] } = useQuery<Provider[]>({
    queryKey: ["providers"],
    queryFn: () => backend("get_providers"),
  });

  const items = data?.items ?? [];
  const total = data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));
  const providerOptions = useMemo(
    () => [
      { value: "", label: isZh ? "全部提供商" : "All Providers" },
      ...providers.map((provider) => ({
        value: provider.name,
        label: provider.name,
      })),
    ],
    [providers, isZh],
  );
  const statusOptions = useMemo(
    () => [
      { value: "", label: isZh ? "全部状态" : "All Status" },
      { value: "ok", label: isZh ? "仅 2xx" : "2xx Only" },
      { value: "error", label: isZh ? "4xx+ 错误" : "4xx+ Errors" },
    ],
    [isZh],
  );
  const providerFilterValue = filter.provider ?? ALL_OPTION;
  const statusFilterValue =
    (filter.status_min ?? null) === 200 && (filter.status_max ?? null) === 299
      ? "ok"
      : (filter.status_min ?? null) === 400 && (filter.status_max ?? null) == null
        ? "error"
        : ALL_OPTION;

  return (
    <div className="space-y-5">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">{isZh ? "请求日志" : "Request Logs"}</h1>
          <p className="mt-1 text-sm text-slate-500">
            {isZh ? `共 ${total} 条记录` : `${total} total records`}
          </p>
        </div>
        <div className="flex gap-2">
          <Select
            value={providerFilterValue}
            onValueChange={(value) => {
              setFilter({ ...filter, provider: value === ALL_OPTION ? undefined : value });
              setPage(0);
            }}
          >
            <SelectTrigger className="w-48">
              <SelectValue placeholder={isZh ? "提供商过滤" : "Provider Filter"} />
            </SelectTrigger>
            <SelectContent>
              {providerOptions.map((option) => (
                <SelectItem key={`provider-${option.value || "all"}`} value={option.value || ALL_OPTION}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select
            value={statusFilterValue}
            onValueChange={(next) => {
              if (next === "error") {
                setFilter({ ...filter, status_min: 400, status_max: undefined });
              } else if (next === "ok") {
                setFilter({ ...filter, status_min: 200, status_max: 299 });
              } else {
                setFilter({ ...filter, status_min: undefined, status_max: undefined });
              }
              setPage(0);
            }}
          >
            <SelectTrigger className="w-44">
              <SelectValue placeholder={isZh ? "状态过滤" : "Status Filter"} />
            </SelectTrigger>
            <SelectContent>
              {statusOptions.map((option) => (
                <SelectItem key={`status-${option.value || "all"}`} value={option.value || ALL_OPTION}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {isLoading ? (
        <div className="text-center text-sm text-slate-500 py-12">{isZh ? "加载中..." : "Loading..."}</div>
      ) : items.length === 0 ? (
        <div className="glass rounded-2xl p-12 text-center">
          <ScrollText className="mx-auto h-10 w-10 text-slate-400" />
          <p className="mt-3 text-sm text-slate-500">{isZh ? "暂无日志" : "No logs yet"}</p>
        </div>
      ) : (
        <div className="glass overflow-hidden rounded-2xl">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="border-b border-slate-200/80 bg-slate-50/50 text-slate-500">
                <tr>
                  <th className="px-4 py-3 text-left font-medium">{isZh ? "时间" : "Time"}</th>
                  <th className="px-4 py-3 text-left font-medium">{isZh ? "模型" : "Model"}</th>
                  <th className="px-4 py-3 text-left font-medium">{isZh ? "提供商" : "Provider"}</th>
                  <th className="px-4 py-3 text-left font-medium">{isZh ? "协议" : "Protocol"}</th>
                  <th className="px-4 py-3 text-center font-medium">{isZh ? "状态" : "Status"}</th>
                  <th className="px-4 py-3 text-right font-medium">{isZh ? "延迟" : "Latency"}</th>
                  <th className="px-4 py-3 text-right font-medium">{isZh ? "Token" : "Tokens"}</th>
                  <th className="px-4 py-3 text-center font-medium">{isZh ? "流式" : "Stream"}</th>
                </tr>
              </thead>
              <tbody>
                {items.map((log) => (
                  <tr key={log.id} className="border-t border-slate-100 text-slate-700 hover:bg-slate-50/50">
                    <td className="px-4 py-2.5 text-xs text-slate-500 whitespace-nowrap">
                      {log.created_at?.replace("T", " ").slice(0, 19)}
                    </td>
                    <td className="px-4 py-2.5 font-medium">{log.actual_model ?? "–"}</td>
                    <td className="px-4 py-2.5">{log.provider_name ?? "–"}</td>
                    <td className="px-4 py-2.5">
                      <span className="text-xs text-slate-500">
                        {log.ingress_protocol} → {log.egress_protocol}
                      </span>
                    </td>
                    <td className="px-4 py-2.5 text-center">
                      <span className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${
                        (log.status_code ?? 0) < 400
                          ? "bg-green-50 text-green-700"
                          : "bg-red-50 text-red-600"
                      }`}>
                        {log.status_code ?? "–"}
                      </span>
                    </td>
                    <td className="px-4 py-2.5 text-right text-xs">
                      {log.duration_ms != null ? `${log.duration_ms.toFixed(0)}ms` : "–"}
                    </td>
                    <td className="px-4 py-2.5 text-right text-xs">
                      {log.input_tokens + log.output_tokens}
                    </td>
                    <td className="px-4 py-2.5 text-center text-xs">
                      {log.is_stream ? "SSE" : "–"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="flex items-center justify-between border-t border-slate-200/80 px-4 py-3">
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
        </div>
      )}
    </div>
  );
}
