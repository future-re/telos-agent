import { useMemo, useState } from "react";
import { Check, Loader2, PencilLine, ShieldAlert, X } from "lucide-react";
import { PendingApproval } from "@/useApprovals";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";

export function ApprovalDialog({
  approval,
  draft,
  error,
  onApprove,
  onDeny,
  onDraftChange,
  onModify,
}: {
  approval?: PendingApproval;
  draft: string;
  error: string;
  onApprove: () => void;
  onDeny: () => void;
  onDraftChange: (value: string) => void;
  onModify: () => void;
}) {
  const [submitting, setSubmitting] = useState<"allow" | "deny" | "modify" | null>(
    null,
  );
  const preview = useMemo(
    () => buildApprovalPreview(approval?.arguments),
    [approval?.arguments],
  );

  function submit(action: "allow" | "deny" | "modify", handler: () => void) {
    if (action === "modify") {
      handler();
      return;
    }
    setSubmitting(action);
    handler();
  }

  return (
    <Dialog
      open={Boolean(approval)}
      onOpenChange={(open) => {
        if (!open && approval) {
          onDeny();
        }
      }}
    >
      <DialogContent
        className="max-h-[min(720px,calc(100vh-2rem))] max-w-3xl grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden p-0"
        onEscapeKeyDown={(event) => event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        <DialogHeader className="border-b px-5 py-4">
          <DialogTitle className="flex min-w-0 items-center gap-2 text-base">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-md border bg-amber-50 text-amber-700">
              <ShieldAlert className="size-4" aria-hidden="true" />
            </span>
            <span className="truncate">确认工具调用</span>
            {approval?.toolName ? (
              <Badge variant="outline" className="shrink-0">
                {approval.toolName}
              </Badge>
            ) : null}
          </DialogTitle>
          <DialogDescription className="pl-10">
            这个调用暂停在这里，批准后会立刻进入执行状态。
          </DialogDescription>
        </DialogHeader>

        {approval && (
          <div className="min-h-0 overflow-y-auto px-5 py-4">
            <div className="grid gap-3">
              <div className="grid gap-2 rounded-lg border bg-muted/30 p-3">
                <div className="flex min-w-0 items-center justify-between gap-3">
                  <span className="text-xs font-semibold uppercase text-muted-foreground">
                    将要执行
                  </span>
                  <span className="rounded-full border bg-background px-2 py-0.5 text-[11px] text-muted-foreground">
                    等待确认
                  </span>
                </div>
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-md bg-zinc-950 px-3 py-2.5 font-mono text-[13px] leading-6 text-zinc-100">
                  {preview.command ?? preview.summary}
                </pre>
              </div>

              <div className="grid gap-2 sm:grid-cols-2">
                <InfoBlock label="原因" value={approval.reason || "需要人工确认"} />
                {approval.cwd ? (
                  <InfoBlock label="工作目录" value={approval.cwd} mono />
                ) : null}
              </div>

              <label className="grid gap-1.5">
                <span className="text-sm font-medium">可编辑参数</span>
                <Textarea
                  className="min-h-36 resize-y font-mono text-xs leading-5"
                  value={draft}
                  onChange={(event) => onDraftChange(event.target.value)}
                  spellCheck={false}
                />
              </label>
              {error ? <p className="text-sm text-red-600">{error}</p> : null}
            </div>
          </div>
        )}

        <DialogFooter className="border-t bg-muted/20 px-5 py-4">
          <Button
            type="button"
            variant="ghost"
            disabled={submitting !== null}
            onClick={() => submit("deny", onDeny)}
          >
            {submitting === "deny" ? (
              <Loader2 className="animate-spin" aria-hidden="true" />
            ) : (
              <X aria-hidden="true" />
            )}
            拒绝
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={submitting !== null}
            onClick={() => submit("modify", onModify)}
          >
            {submitting === "modify" ? (
              <Loader2 className="animate-spin" aria-hidden="true" />
            ) : (
              <PencilLine aria-hidden="true" />
            )}
            按编辑参数批准
          </Button>
          <Button
            type="button"
            disabled={submitting !== null}
            onClick={() => submit("allow", onApprove)}
          >
            {submitting === "allow" ? (
              <Loader2 className="animate-spin" aria-hidden="true" />
            ) : (
              <Check aria-hidden="true" />
            )}
            {submitting === "allow" ? "正在提交" : "批准执行"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function InfoBlock({
  label,
  mono,
  value,
}: {
  label: string;
  mono?: boolean;
  value: string;
}) {
  return (
    <div className="min-w-0 rounded-md border bg-background px-3 py-2.5 text-sm">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <span
        className={`mt-1 block break-words ${mono ? "font-mono text-xs" : ""}`}
      >
        {value}
      </span>
    </div>
  );
}

function buildApprovalPreview(argumentsValue: unknown): {
  command?: string;
  summary: string;
} {
  if (argumentsValue && typeof argumentsValue === "object") {
    const args = argumentsValue as Record<string, unknown>;
    if (typeof args.command === "string" && args.command.trim()) {
      return { command: args.command, summary: args.command };
    }
    if (typeof args.file_path === "string" && args.file_path.trim()) {
      return { summary: args.file_path };
    }
  }

  try {
    return { summary: JSON.stringify(argumentsValue ?? {}, null, 2) };
  } catch {
    return { summary: "无法预览参数" };
  }
}
