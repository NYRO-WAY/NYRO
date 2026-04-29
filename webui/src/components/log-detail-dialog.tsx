import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Check, Copy, Loader2 } from "lucide-react";

import { backend } from "@/lib/backend";
import { useLocale } from "@/lib/i18n";
import type { RequestLog } from "@/lib/types";
import { formatDuration, formatLogTime, formatTokenCount, tryPrettyJson } from "@/lib/format";
import { prettyName } from "@/lib/protocol-id";
import { cn } from "@/lib/utils";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

interface LogDetailDialogProps {
  logId: string | null;
  summary?: RequestLog | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function LogDetailDialog({ logId, summary, open, onOpenChange }: LogDetailDialogProps) {
  const { locale } = useLocale();
  const isZh = locale === "zh-CN";

  const { data, isLoading } = useQuery<RequestLog | null>({
    queryKey: ["log-detail", logId],
    queryFn: () => backend("get_log", { id: logId! }),
    enabled: open && !!logId,
  });

  const log = data ?? summary ?? null;

  const method = log?.method ?? "–";
  const path = log?.path ?? "–";
  const statusCode = log?.status_code;
  const statusOk = (statusCode ?? 0) < 400;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[min(92vw,960px)] max-h-[88vh] overflow-hidden flex flex-col gap-4">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <span>{isZh ? "请求详情" : "Request Detail"}</span>
            {isLoading ? <Loader2 className="h-3.5 w-3.5 animate-spin text-slate-400" /> : null}
          </DialogTitle>
          <DialogDescription>
            {log ? formatLogTime(log.created_at) : ""}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-wrap items-center gap-2 text-xs">
          <Badge variant="outline" className="font-mono">{method}</Badge>
          <span className="font-mono text-slate-600 break-all">{path}</span>
          <span
            className={cn(
              "inline-flex rounded-full px-2 py-0.5 font-medium",
              statusOk ? "bg-green-50 text-green-700" : "bg-red-50 text-red-600",
            )}
          >
            {statusCode ?? "–"}
          </span>
          {log?.is_stream ? (
            <Badge variant="outline" className="border-green-200 bg-green-50 text-green-700">
              SSE
            </Badge>
          ) : (
            <Badge variant="outline" className="border-sky-200 bg-sky-50 text-sky-700">
              JSON
            </Badge>
          )}
          {log?.provider_name ? (
            <Badge variant="outline">{log.provider_name}</Badge>
          ) : null}
          {log?.actual_model ? (
            <span className="text-slate-500 font-mono">{log.actual_model}</span>
          ) : null}
          {log?.duration_ms != null ? (
            <span className="text-slate-500">{formatDuration(log.duration_ms)}</span>
          ) : null}
          {log ? (
            <span className="inline-flex items-center gap-2">
              <span className="inline-flex items-center gap-1 text-sky-600">
                <span className="text-[10px] font-semibold tracking-wide">IN</span>
                <span title={String(log.input_tokens)}>
                  {formatTokenCount(log.input_tokens)}
                </span>
              </span>
              <span className="inline-flex items-center gap-1 text-emerald-600">
                <span className="text-[10px] font-semibold tracking-wide">OUT</span>
                <span title={String(log.output_tokens)}>
                  {formatTokenCount(log.output_tokens)}
                </span>
              </span>
            </span>
          ) : null}
          {log?.ingress_protocol || log?.egress_protocol ? (
            <ProtocolLine ingress={log?.ingress_protocol} egress={log?.egress_protocol} />
          ) : null}
        </div>

        {log?.error_message ? (
          <div className="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-700">
            {log.error_message}
          </div>
        ) : null}

        <div className="flex-1 space-y-3 overflow-y-auto pr-1">
          <PayloadBlock
            title={isZh ? "请求头" : "Request Headers"}
            content={log?.request_headers}
            isZh={isZh}
          />
          <PayloadBlock
            title={isZh ? "请求体" : "Request Body"}
            content={log?.request_body}
            isZh={isZh}
          />
          <PayloadBlock
            title={isZh ? "响应头" : "Response Headers"}
            content={log?.response_headers}
            isZh={isZh}
          />
          <PayloadBlock
            title={isZh ? "响应体" : "Response Body"}
            content={log?.response_body}
            isZh={isZh}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}

interface PayloadBlockProps {
  title: string;
  content: string | null | undefined;
  isZh: boolean;
}

function PayloadBlock({ title, content, isZh }: PayloadBlockProps) {
  const [copied, setCopied] = useState(false);
  const pretty = tryPrettyJson(content);
  const hasContent = !!(content && content.trim());

  useEffect(() => {
    if (!copied) return;
    const t = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(t);
  }, [copied]);

  const handleCopy = async () => {
    if (!hasContent) return;
    try {
      await navigator.clipboard.writeText(pretty);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="rounded-lg border border-slate-200 bg-slate-50/60">
      <div className="flex items-center justify-between border-b border-slate-200 px-3 py-1.5">
        <span className="text-xs font-medium text-slate-600">{title}</span>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={!hasContent}
          onClick={handleCopy}
          className="h-7 gap-1 px-2 text-xs"
        >
          {copied ? (
            <>
              <Check className="h-3.5 w-3.5" />
              {isZh ? "已复制" : "Copied"}
            </>
          ) : (
            <>
              <Copy className="h-3.5 w-3.5" />
              {isZh ? "复制" : "Copy"}
            </>
          )}
        </Button>
      </div>
      <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-[11px] leading-relaxed text-slate-700">
        {hasContent ? pretty : <span className="text-slate-400">{isZh ? "（无内容）" : "(empty)"}</span>}
      </pre>
    </div>
  );
}

function ProtocolLine({
  ingress,
  egress,
}: {
  ingress: string | null | undefined;
  egress: string | null | undefined;
}) {
  const inPretty = prettyName(ingress);
  const outPretty = prettyName(egress);
  const renderSide = (raw: string | null | undefined, p: ReturnType<typeof prettyName>) => {
    if (!raw) return <span className="text-slate-400">–</span>;
    if (!p) return <span className="text-slate-500">{raw}</span>;
    return (
      <span className="inline-flex flex-col leading-tight">
        <span className="font-medium text-slate-600">{p.family}</span>
        <span className="text-[10px] text-slate-400">{p.detail}</span>
      </span>
    );
  };
  return (
    <span className="inline-flex items-center gap-1.5 text-slate-500">
      {renderSide(ingress, inPretty)}
      <span className="text-slate-300">→</span>
      {renderSide(egress, outPretty)}
    </span>
  );
}
