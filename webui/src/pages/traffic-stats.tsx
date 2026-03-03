import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Header } from "@/components/layout/header";
import { api, type MetricsDimension } from "@/lib/api";

type ViewMode = "api" | "ai";

function formatNum(value: number) {
  return new Intl.NumberFormat("zh-CN").format(value);
}

function MetricTable({
  title,
  items,
}: {
  title: string;
  items: MetricsDimension[];
}) {
  const rows = useMemo(
    () => [...items].sort((a, b) => b.requests - a.requests).slice(0, 10),
    [items],
  );

  return (
    <div className="glass rounded-2xl p-4">
      <div className="mb-3 text-sm font-semibold text-slate-800">{title}</div>
      <div className="overflow-x-auto rounded-xl border border-white/70 bg-white/55">
        <table className="w-full min-w-[620px] text-[12px]">
          <thead className="bg-white/70 text-slate-500">
            <tr>
              <th className="px-3 py-2 text-left font-medium">Name</th>
              <th className="px-3 py-2 text-right font-medium">Req</th>
              <th className="px-3 py-2 text-right font-medium">Latency</th>
              <th className="px-3 py-2 text-right font-medium">2xx</th>
              <th className="px-3 py-2 text-right font-medium">4xx</th>
              <th className="px-3 py-2 text-right font-medium">5xx</th>
              <th className="px-3 py-2 text-right font-medium">Input</th>
              <th className="px-3 py-2 text-right font-medium">Output</th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr>
                <td className="px-3 py-8 text-center text-slate-500" colSpan={8}>
                  暂无数据
                </td>
              </tr>
            )}
            {rows.map((row) => (
              <tr key={row.name} className="border-t border-white/70 text-slate-700">
                <td className="px-3 py-2 font-medium">{row.name}</td>
                <td className="px-3 py-2 text-right">{formatNum(row.requests)}</td>
                <td className="px-3 py-2 text-right">{row.latency_avg_ms?.toFixed(2) || "-"}</td>
                <td className="px-3 py-2 text-right">{formatNum(row.status["2xx"] || 0)}</td>
                <td className="px-3 py-2 text-right">{formatNum(row.status["4xx"] || 0)}</td>
                <td className="px-3 py-2 text-right">{formatNum(row.status["5xx"] || 0)}</td>
                <td className="px-3 py-2 text-right">{formatNum(row.input_tokens || 0)}</td>
                <td className="px-3 py-2 text-right">{formatNum(row.output_tokens || 0)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default function TrafficStatsPage() {
  const [mode, setMode] = useState<ViewMode>("api");

  const metricsQuery = useQuery({
    queryKey: ["metrics"],
    queryFn: api.getMetrics,
    refetchInterval: 4000,
  });

  const metrics = metricsQuery.data;

  return (
    <div className="space-y-5">
      <Header
        title="Traffic Stats"
        subtitle="Real-time metrics from /nyro/local/metrics"
        onRefresh={() => void metricsQuery.refetch()}
        isRefreshing={metricsQuery.isFetching}
        actions={
          <div className="inline-flex items-center gap-1 rounded-full border border-white/70 bg-white/70 p-1">
            <button
              onClick={() => setMode("api")}
              className={`rounded-full px-3 py-1 text-xs ${mode === "api" ? "bg-slate-900 text-white" : "text-slate-600"}`}
            >
              API
            </button>
            <button
              onClick={() => setMode("ai")}
              className={`rounded-full px-3 py-1 text-xs ${mode === "ai" ? "bg-slate-900 text-white" : "text-slate-600"}`}
            >
              AI
            </button>
          </div>
        }
      />

      <div className="grid grid-cols-1 gap-4 md:grid-cols-4">
        <div className="glass rounded-2xl p-4">
          <p className="text-xs text-slate-500">Total Requests</p>
          <p className="mt-2 text-2xl font-semibold text-slate-900">
            {formatNum(metrics?.total_requests || 0)}
          </p>
        </div>
        <div className="glass rounded-2xl p-4">
          <p className="text-xs text-slate-500">Active Connections</p>
          <p className="mt-2 text-2xl font-semibold text-slate-900">
            {formatNum(metrics?.active_connections || 0)}
          </p>
        </div>
        <div className="glass rounded-2xl p-4">
          <p className="text-xs text-slate-500">Input Tokens</p>
          <p className="mt-2 text-2xl font-semibold text-slate-900">
            {formatNum(metrics?.total_input_tokens || 0)}
          </p>
        </div>
        <div className="glass rounded-2xl p-4">
          <p className="text-xs text-slate-500">Output Tokens</p>
          <p className="mt-2 text-2xl font-semibold text-slate-900">
            {formatNum(metrics?.total_output_tokens || 0)}
          </p>
        </div>
      </div>

      {mode === "api" ? (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
          <MetricTable title="Routes" items={metrics?.routes || []} />
          <MetricTable title="Services" items={metrics?.services || []} />
          <MetricTable title="Consumers" items={metrics?.consumers || []} />
        </div>
      ) : (
        <MetricTable title="Models" items={metrics?.models || []} />
      )}
    </div>
  );
}

