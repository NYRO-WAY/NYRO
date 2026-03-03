import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Header } from "@/components/layout/header";
import { api } from "@/lib/api";

function formatNum(value: number) {
  return new Intl.NumberFormat("zh-CN").format(value);
}

function statusClass(status: number) {
  if (status >= 200 && status < 300) return "bg-emerald-100 text-emerald-700";
  if (status >= 400 && status < 500) return "bg-amber-100 text-amber-700";
  return "bg-rose-100 text-rose-700";
}

export default function TrafficLogsPage() {
  const [limit, setLimit] = useState(100);

  const logsQuery = useQuery({
    queryKey: ["logs", limit],
    queryFn: () => api.getLogs(limit),
    refetchInterval: 4000,
  });

  const data = logsQuery.data;

  return (
    <div className="space-y-5">
      <Header
        title="Traffic Logs"
        subtitle="Real-time request logs from /nyro/local/logs"
        onRefresh={() => void logsQuery.refetch()}
        isRefreshing={logsQuery.isFetching}
        actions={
          <select
            value={limit}
            onChange={(e) => setLimit(Number(e.target.value))}
            className="h-9 rounded-full border border-white/70 bg-white/70 px-3 text-xs text-slate-700 outline-none"
          >
            <option value={50}>50 rows</option>
            <option value={100}>100 rows</option>
            <option value={200}>200 rows</option>
            <option value={500}>500 rows</option>
          </select>
        }
      />

      <div className="glass rounded-2xl p-4">
        <div className="mb-3 text-xs text-slate-500">
          total: {formatNum(data?.total || 0)}
        </div>
        <div className="overflow-x-auto rounded-xl border border-white/70 bg-white/55">
          <table className="min-w-[1100px] w-full text-[12px]">
            <thead className="bg-white/70 text-slate-500">
              <tr>
                <th className="px-3 py-2 text-left font-medium">Status</th>
                <th className="px-3 py-2 text-left font-medium">Method</th>
                <th className="px-3 py-2 text-left font-medium">Model</th>
                <th className="px-3 py-2 text-left font-medium">Service</th>
                <th className="px-3 py-2 text-left font-medium">Route</th>
                <th className="px-3 py-2 text-left font-medium">URI</th>
                <th className="px-3 py-2 text-right font-medium">Input</th>
                <th className="px-3 py-2 text-right font-medium">Output</th>
                <th className="px-3 py-2 text-right font-medium">Latency(ms)</th>
                <th className="px-3 py-2 text-right font-medium">Time</th>
              </tr>
            </thead>
            <tbody>
              {!logsQuery.isLoading && (data?.items.length || 0) === 0 && (
                <tr>
                  <td className="px-3 py-8 text-center text-slate-500" colSpan={10}>
                    暂无日志数据
                  </td>
                </tr>
              )}
              {data?.items.map((item, idx) => (
                <tr key={`${item.request_id}-${idx}`} className="border-t border-white/70 text-slate-700">
                  <td className="px-3 py-2">
                    <span className={`rounded-full px-2 py-0.5 text-[11px] font-semibold ${statusClass(item.status)}`}>
                      {item.status}
                    </span>
                  </td>
                  <td className="px-3 py-2 font-medium">{item.method}</td>
                  <td className="px-3 py-2">{item.model || "-"}</td>
                  <td className="px-3 py-2">{item.service || "-"}</td>
                  <td className="px-3 py-2">{item.route || "-"}</td>
                  <td className="max-w-[260px] truncate px-3 py-2">{item.uri}</td>
                  <td className="px-3 py-2 text-right">{formatNum(item.input_tokens || 0)}</td>
                  <td className="px-3 py-2 text-right">{formatNum(item.output_tokens || 0)}</td>
                  <td className="px-3 py-2 text-right">{item.latency_ms}</td>
                  <td className="px-3 py-2 text-right">{new Date(item.timestamp).toLocaleTimeString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

