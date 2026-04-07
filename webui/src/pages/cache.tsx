import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { backend } from "@/lib/backend";
import { useLocale } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export default function CachePage() {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";
  const qc = useQueryClient();
  const [cacheKey, setCacheKey] = useState("");

  const { data: settings } = useQuery({
    queryKey: ["cache-settings"],
    queryFn: () => backend<Record<string, unknown>>("get_cache_settings"),
  });
  const { data: stats } = useQuery({
    queryKey: ["cache-stats"],
    queryFn: () => backend<Record<string, unknown>>("get_cache_stats"),
  });

  const flushMut = useMutation({
    mutationFn: () => backend("flush_cache"),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["cache-stats"] });
    },
  });

  const deleteMut = useMutation({
    mutationFn: () => backend("delete_cache_key", { key: cacheKey }),
    onSuccess: () => {
      setCacheKey("");
      qc.invalidateQueries({ queryKey: ["cache-stats"] });
    },
  });

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-slate-900">{isZh ? "缓存管理" : "Cache"}</h1>
        <p className="mt-1 text-sm text-slate-500">
          {isZh ? "查看缓存配置与执行清理操作" : "Inspect cache settings and run maintenance actions"}
        </p>
      </div>

      <div className="glass rounded-2xl p-6 space-y-3">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "当前配置" : "Settings"}</h2>
        <pre className="rounded-lg bg-slate-50 p-3 text-xs text-slate-700 overflow-auto">
          {JSON.stringify(settings ?? {}, null, 2)}
        </pre>
      </div>

      <div className="glass rounded-2xl p-6 space-y-3">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "运行状态" : "Runtime stats"}</h2>
        <pre className="rounded-lg bg-slate-50 p-3 text-xs text-slate-700 overflow-auto">
          {JSON.stringify(stats ?? {}, null, 2)}
        </pre>
      </div>

      <div className="glass rounded-2xl p-6 space-y-3">
        <h2 className="text-lg font-semibold text-slate-900">{isZh ? "维护操作" : "Maintenance"}</h2>
        <div className="flex items-center gap-2">
          <Button onClick={() => flushMut.mutate()} disabled={flushMut.isPending}>
            {isZh ? "清空缓存" : "Flush cache"}
          </Button>
          <Input
            placeholder={isZh ? "输入要删除的 cache key" : "Cache key to delete"}
            value={cacheKey}
            onChange={(e) => setCacheKey(e.target.value)}
          />
          <Button
            variant="secondary"
            onClick={() => deleteMut.mutate()}
            disabled={deleteMut.isPending || !cacheKey.trim()}
          >
            {isZh ? "删除 Key" : "Delete key"}
          </Button>
        </div>
      </div>
    </div>
  );
}
