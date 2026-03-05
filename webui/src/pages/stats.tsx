import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis, PieChart, Pie, Cell } from "recharts";
import { backend } from "@/lib/backend";
import type { StatsOverview, StatsHourly, ModelStats, ProviderStats } from "@/lib/types";
import { Zap, Clock, Activity } from "lucide-react";
import { useLocale } from "@/lib/i18n";

const COLORS = ["#3b82f6", "#10b981", "#f59e0b", "#ef4444", "#8b5cf6", "#ec4899", "#06b6d4", "#84cc16"];

function fmt(n: number) {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return String(n);
}

function fmtLatency(ms: number) {
  if (ms >= 1000) {
    return `${(ms / 1000).toFixed(ms >= 10_000 ? 1 : 2)}s`;
  }
  return `${ms.toFixed(0)}ms`;
}

export default function StatsPage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";

  const [hours, setHours] = useState(24);

  const { data: overview } = useQuery<StatsOverview>({
    queryKey: ["stats-overview"],
    queryFn: () => backend("get_stats_overview"),
    refetchInterval: 10_000,
  });

  const { data: hourly = [] } = useQuery<StatsHourly[]>({
    queryKey: ["stats-hourly", hours],
    queryFn: () => backend("get_stats_hourly", { hours }),
    refetchInterval: 30_000,
  });

  const { data: modelStats = [] } = useQuery<ModelStats[]>({
    queryKey: ["stats-models"],
    queryFn: () => backend("get_stats_by_model"),
    refetchInterval: 30_000,
  });

  const { data: providerStats = [] } = useQuery<ProviderStats[]>({
    queryKey: ["stats-providers"],
    queryFn: () => backend("get_stats_by_provider"),
    refetchInterval: 30_000,
  });

  const tokenChart = hourly.map((h) => ({
    hour: h.hour.slice(11, 16),
    input: h.total_input_tokens,
    output: h.total_output_tokens,
  }));

  const modelPie = modelStats.slice(0, 6).map((m) => ({
    name: m.model,
    value: m.request_count,
  }));
  const modelTotal = modelPie.reduce((acc, m) => acc + m.value, 0);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">{isZh ? "统计" : "Statistics"}</h1>
          <p className="mt-1 text-sm text-slate-500">
            {isZh ? "Token 使用、延迟与错误分析" : "Token usage, latency, and error analytics"}
          </p>
        </div>
        <select
          value={hours}
          onChange={(e) => setHours(Number(e.target.value))}
          className="rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm outline-none"
        >
          <option value={6}>{isZh ? "最近 6 小时" : "Last 6h"}</option>
          <option value={24}>{isZh ? "最近 24 小时" : "Last 24h"}</option>
          <option value={72}>{isZh ? "最近 3 天" : "Last 3d"}</option>
          <option value={168}>{isZh ? "最近 7 天" : "Last 7d"}</option>
        </select>
      </div>

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {[
          { label: isZh ? "总请求数" : "Total Requests", value: fmt(overview?.total_requests ?? 0), icon: Activity, color: "text-blue-600" },
          { label: isZh ? "输入 Token" : "Input Tokens", value: fmt(overview?.total_input_tokens ?? 0), icon: Zap, color: "text-amber-600" },
          { label: isZh ? "输出 Token" : "Output Tokens", value: fmt(overview?.total_output_tokens ?? 0), icon: Zap, color: "text-green-600" },
          { label: isZh ? "平均延迟" : "Avg Latency", value: `${(overview?.avg_duration_ms ?? 0).toFixed(0)}ms`, icon: Clock, color: "text-purple-600" },
        ].map((c) => (
          <div key={c.label} className="glass rounded-2xl p-4">
            <div className="flex items-center gap-2">
              <c.icon className={`h-4 w-4 ${c.color}`} />
              <p className="text-xs font-medium text-slate-500">{c.label}</p>
            </div>
            <p className="mt-1.5 text-[24px] leading-none font-semibold text-slate-900">
              {c.label === (isZh ? "平均延迟" : "Avg Latency") ? fmtLatency(overview?.avg_duration_ms ?? 0) : c.value}
            </p>
          </div>
        ))}
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <div className="glass rounded-2xl p-6">
          <h3 className="mb-4 text-sm font-semibold text-slate-800">{isZh ? "Token 时序" : "Token Usage Over Time"}</h3>
          <div className="h-48">
            {tokenChart.length > 0 ? (
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={tokenChart}>
                  <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#e2e8f0" />
                  <XAxis dataKey="hour" tick={{ fill: "#64748b", fontSize: 11 }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fill: "#64748b", fontSize: 11 }} axisLine={false} tickLine={false} width={50} tickFormatter={fmt} />
                  <Tooltip />
                  <Bar dataKey="input" name={isZh ? "输入" : "Input"} stackId="a" fill="#3b82f6" />
                  <Bar dataKey="output" name={isZh ? "输出" : "Output"} stackId="a" fill="#10b981" radius={[4, 4, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-slate-400">{isZh ? "暂无数据" : "No data"}</div>
            )}
          </div>
        </div>

        <div className="glass rounded-2xl p-6">
          <h3 className="mb-4 text-sm font-semibold text-slate-800">{isZh ? "模型请求分布" : "Requests by Model"}</h3>
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-[240px_1fr] lg:items-center">
            <div className="h-44">
            {modelPie.length > 0 ? (
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={modelPie}
                      cx="50%"
                      cy="50%"
                      innerRadius={44}
                      outerRadius={68}
                      paddingAngle={3}
                      dataKey="value"
                      label={false}
                      labelLine={false}
                    >
                      {modelPie.map((_, i) => (
                        <Cell key={i} fill={COLORS[i % COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip />
                  </PieChart>
                </ResponsiveContainer>
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-slate-400">{isZh ? "暂无数据" : "No data"}</div>
            )}
            </div>
            {modelPie.length > 0 && (
              <div className="space-y-2">
                {modelPie.map((item, i) => {
                  const pct = modelTotal > 0 ? Math.round((item.value / modelTotal) * 100) : 0;
                  return (
                    <div key={item.name} className="flex items-center justify-between gap-3 text-sm">
                      <div className="min-w-0 flex items-center gap-2">
                        <span
                          className="h-2.5 w-2.5 shrink-0 rounded-full"
                          style={{ backgroundColor: COLORS[i % COLORS.length] }}
                        />
                        <span className="truncate text-slate-600">{item.name}</span>
                      </div>
                      <span className="shrink-0 font-medium text-slate-900">{pct}%</span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </div>

      <div className="glass rounded-2xl p-6">
        <h3 className="mb-4 text-sm font-semibold text-slate-800">{isZh ? "提供商分布" : "Provider Breakdown"}</h3>
        <div className="overflow-hidden rounded-xl border border-white/70 bg-white/50">
          <table className="w-full text-sm">
            <thead className="bg-white/70 text-slate-500">
              <tr>
                <th className="px-4 py-2.5 text-left font-medium">{isZh ? "提供商" : "Provider"}</th>
                <th className="px-4 py-2.5 text-right font-medium">{isZh ? "请求数" : "Requests"}</th>
                <th className="px-4 py-2.5 text-right font-medium">{isZh ? "错误数" : "Errors"}</th>
                <th className="px-4 py-2.5 text-right font-medium">{isZh ? "错误率" : "Error Rate"}</th>
                <th className="px-4 py-2.5 text-right font-medium">{isZh ? "平均延迟" : "Avg Latency"}</th>
              </tr>
            </thead>
            <tbody>
              {providerStats.length === 0 && (
                <tr><td className="px-4 py-6 text-center text-slate-400" colSpan={5}>{isZh ? "暂无数据" : "No data"}</td></tr>
              )}
              {providerStats.slice(0, 8).map((p) => (
                <tr key={p.provider} className="border-t border-white/70 text-slate-700">
                  <td className="px-4 py-2.5 font-medium">{p.provider}</td>
                  <td className="px-4 py-2.5 text-right">{fmt(p.request_count)}</td>
                  <td className="px-4 py-2.5 text-right text-red-500">{p.error_count}</td>
                  <td className="px-4 py-2.5 text-right">
                    {p.request_count > 0 ? ((p.error_count / p.request_count) * 100).toFixed(1) : "0"}%
                  </td>
                  <td className="px-4 py-2.5 text-right">{fmtLatency(p.avg_duration_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
