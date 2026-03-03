import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis } from "recharts";
import { Header } from "@/components/layout/header";
import { api } from "@/lib/api";

function formatNum(value: number) {
  return new Intl.NumberFormat("zh-CN").format(value);
}

function formatUptime(seconds: number) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

export default function DashboardPage() {
  const statusQuery = useQuery({
    queryKey: ["status"],
    queryFn: api.getStatus,
    refetchInterval: 5000,
  });

  const metricsQuery = useQuery({
    queryKey: ["metrics"],
    queryFn: api.getMetrics,
    refetchInterval: 5000,
  });

  const loading = statusQuery.isLoading || metricsQuery.isLoading;
  const status = statusQuery.data?.data;
  const metrics = metricsQuery.data;

  const statusChart = useMemo(() => {
    if (!metrics) {
      return [
        { name: "2xx", value: 0 },
        { name: "4xx", value: 0 },
        { name: "5xx", value: 0 },
      ];
    }
    const agg = { "2xx": 0, "4xx": 0, "5xx": 0 };
    for (const item of metrics.routes) {
      agg["2xx"] += item.status["2xx"] || 0;
      agg["4xx"] += item.status["4xx"] || 0;
      agg["5xx"] += item.status["5xx"] || 0;
    }
    return [
      { name: "2xx", value: agg["2xx"] },
      { name: "4xx", value: agg["4xx"] },
      { name: "5xx", value: agg["5xx"] },
    ];
  }, [metrics]);

  const topModels = useMemo(() => {
    if (!metrics) return [];
    return [...metrics.models].sort((a, b) => b.requests - a.requests).slice(0, 6);
  }, [metrics]);

  const refreshAll = () => {
    void statusQuery.refetch();
    void metricsQuery.refetch();
  };

  return (
    <div className="space-y-5">
      <Header
        title="Dashboard"
        subtitle="Professional AI/API Gateway Console"
        onRefresh={refreshAll}
        isRefreshing={statusQuery.isFetching || metricsQuery.isFetching}
      />

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {[
          {
            label: "Total Requests",
            value: metrics ? formatNum(metrics.total_requests) : "—",
          },
          {
            label: "Active Connections",
            value: metrics ? formatNum(metrics.active_connections) : "—",
          },
          {
            label: "Input Tokens",
            value: metrics ? formatNum(metrics.total_input_tokens) : "—",
          },
          {
            label: "Output Tokens",
            value: metrics ? formatNum(metrics.total_output_tokens) : "—",
          },
          {
            label: "Config Version",
            value: status ? String(status.config_version) : "—",
          },
          {
            label: "Store Mode",
            value: status?.store_mode || "—",
          },
          {
            label: "Workers",
            value: status ? String(status.worker_count) : "—",
          },
          {
            label: "Uptime",
            value: status ? formatUptime(status.uptime_seconds) : "—",
          },
        ].map((card) => (
          <div
            key={card.label}
            className="glass rounded-2xl p-5 transition-all duration-200 hover:-translate-y-0.5 hover:shadow-xl cursor-pointer"
          >
            <p className="text-sm font-medium text-slate-500">
              {card.label}
            </p>
            <p className="mt-2 text-2xl font-semibold text-slate-900">
              {card.value}
            </p>
          </div>
        ))}
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <div className="glass rounded-2xl p-6">
          <div className="mb-4 flex items-center justify-between">
            <h3 className="text-sm font-semibold text-slate-800">Status Distribution</h3>
            {loading && <span className="text-xs text-slate-500">Loading...</span>}
          </div>
          <div className="h-64">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={statusChart}>
                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#dbe4f0" />
                <XAxis dataKey="name" tick={{ fill: "#64748b", fontSize: 12 }} axisLine={false} tickLine={false} />
                <Tooltip />
                <Bar dataKey="value" radius={[8, 8, 0, 0]} fill="#1e40af" />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>

        <div className="glass rounded-2xl p-6">
          <div className="mb-4 flex items-center justify-between">
            <h3 className="text-sm font-semibold text-slate-800">Top Models</h3>
            <span className="text-xs text-slate-500">{topModels.length} models</span>
          </div>

          <div className="overflow-hidden rounded-xl border border-white/70 bg-white/50">
            <table className="w-full text-sm">
              <thead className="bg-white/70 text-slate-500">
                <tr>
                  <th className="px-3 py-2 text-left font-medium">Model</th>
                  <th className="px-3 py-2 text-right font-medium">Req</th>
                  <th className="px-3 py-2 text-right font-medium">Input</th>
                  <th className="px-3 py-2 text-right font-medium">Output</th>
                </tr>
              </thead>
              <tbody>
                {topModels.length === 0 && (
                  <tr>
                    <td className="px-3 py-6 text-center text-slate-500" colSpan={4}>
                      暂无 AI 模型数据
                    </td>
                  </tr>
                )}
                {topModels.map((m) => (
                  <tr key={m.name} className="border-t border-white/70 text-slate-700">
                    <td className="px-3 py-2 font-medium">{m.name}</td>
                    <td className="px-3 py-2 text-right">{formatNum(m.requests)}</td>
                    <td className="px-3 py-2 text-right">{formatNum(m.input_tokens)}</td>
                    <td className="px-3 py-2 text-right">{formatNum(m.output_tokens)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
