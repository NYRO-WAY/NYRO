import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { backend } from "@/lib/backend";
import type { LogPage, LogQuery } from "@/lib/types";
import { ScrollText, ChevronLeft, ChevronRight } from "lucide-react";
import { useLocale } from "@/lib/i18n";

const PAGE_SIZE = 12;

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

  const items = data?.items ?? [];
  const total = data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));

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
          <select
            value={filter.provider ?? ""}
            onChange={(e) => { setFilter({ ...filter, provider: e.target.value || undefined }); setPage(0); }}
            className="rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm outline-none"
          >
            <option value="">{isZh ? "全部提供商" : "All Providers"}</option>
          </select>
          <select
            value={filter.status_min != null ? String(filter.status_min) : ""}
            onChange={(e) => {
              const v = e.target.value;
              if (v === "error") {
                setFilter({ ...filter, status_min: 400, status_max: undefined });
              } else if (v === "ok") {
                setFilter({ ...filter, status_min: 200, status_max: 299 });
              } else {
                setFilter({ ...filter, status_min: undefined, status_max: undefined });
              }
              setPage(0);
            }}
            className="rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm outline-none"
          >
            <option value="">{isZh ? "全部状态" : "All Status"}</option>
            <option value="ok">{isZh ? "仅 2xx" : "2xx Only"}</option>
            <option value="error">{isZh ? "4xx+ 错误" : "4xx+ Errors"}</option>
          </select>
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
              <button
                onClick={() => setPage(Math.max(0, page - 1))}
                disabled={page === 0}
                className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 disabled:opacity-30 cursor-pointer"
              >
                <ChevronLeft className="h-4 w-4" />
              </button>
              <button
                onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
                disabled={page >= totalPages - 1}
                className="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 disabled:opacity-30 cursor-pointer"
              >
                <ChevronRight className="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
