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
        className="max-w-2xl"
        onEscapeKeyDown={(event) => event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            命令需要确认
            {approval?.toolName ? (
              <Badge variant="outline">{approval.toolName}</Badge>
            ) : null}
          </DialogTitle>
          <DialogDescription>
            自动批准关闭时，需审批的工具调用会暂停在这里等待你的决定。
          </DialogDescription>
        </DialogHeader>

        {approval && (
          <div className="grid gap-3">
            <div className="grid gap-1 rounded-md border bg-muted/30 p-3 text-sm">
              <span className="font-medium">原因</span>
              <span className="text-muted-foreground">
                {approval.reason || "需要人工确认"}
              </span>
            </div>
            {approval.cwd ? (
              <div className="grid gap-1 rounded-md border bg-muted/30 p-3 text-sm">
                <span className="font-medium">工作目录</span>
                <span className="break-all font-mono text-xs text-muted-foreground">
                  {approval.cwd}
                </span>
              </div>
            ) : null}
            <label className="grid gap-1.5">
              <span className="text-sm font-medium">调用参数</span>
              <Textarea
                className="min-h-44 font-mono text-xs leading-5"
                value={draft}
                onChange={(event) => onDraftChange(event.target.value)}
                spellCheck={false}
              />
            </label>
            {error ? <p className="text-sm text-red-600">{error}</p> : null}
          </div>
        )}

        <DialogFooter>
          <Button type="button" variant="destructive" onClick={onDeny}>
            拒绝
          </Button>
          <Button type="button" variant="outline" onClick={onModify}>
            按编辑参数批准
          </Button>
          <Button type="button" onClick={onApprove}>
            批准
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
