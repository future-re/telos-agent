import { Database, FolderOpen } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { MemoryBucket, MemoryOverview } from "@/desktopTypes";

interface MemoryOverviewDialogProps {
  loading: boolean;
  memory?: MemoryOverview;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function MemoryOverviewDialog({
  loading,
  memory,
  onOpenChange,
  open,
}: MemoryOverviewDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Database className="size-5" aria-hidden="true" />
            记忆
          </DialogTitle>
          <DialogDescription>
            当前工作区使用的长期记忆。这里是只读视图，编辑仍通过记忆工具完成。
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="rounded-md border border-dashed bg-muted/40 p-5 text-sm text-muted-foreground">
            正在读取记忆...
          </div>
        ) : !memory ? (
          <div className="rounded-md border border-dashed bg-muted/40 p-5 text-sm text-muted-foreground">
            暂时无法读取记忆。
          </div>
        ) : (
          <div className="grid gap-4">
            <section className="grid gap-3 rounded-md border bg-muted/25 p-3 sm:grid-cols-[160px_minmax(0,1fr)]">
              <div>
                <span className="text-xs text-muted-foreground">总量</span>
                <strong className="mt-1 block text-3xl leading-none">{memory.total}</strong>
              </div>
              <div className="min-w-0">
                <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <FolderOpen className="size-3.5" aria-hidden="true" />
                  存储位置
                </span>
                <strong className="mt-1 block truncate text-sm font-medium" title={memory.root}>
                  {memory.root}
                </strong>
              </div>
            </section>

            <section className="grid gap-3 sm:grid-cols-2">
              <BucketChart title="分类" buckets={memory.categories} total={memory.total} />
              <BucketChart title="状态" buckets={memory.statuses} total={memory.total} />
            </section>

            <section className="grid gap-2">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-semibold">最近更新</h3>
                <span className="text-xs text-muted-foreground">{memory.recent.length} 条</span>
              </div>
              <ScrollArea className="max-h-[320px] rounded-md border">
                {memory.recent.length === 0 ? (
                  <div className="p-4 text-sm text-muted-foreground">还没有记忆。</div>
                ) : (
                  <div className="divide-y">
                    {memory.recent.map((entry) => (
                      <article key={entry.name} className="grid gap-2 p-3">
                        <div className="flex min-w-0 items-start justify-between gap-3">
                          <div className="min-w-0">
                            <strong className="block truncate text-sm" title={entry.name}>
                              {entry.name}
                            </strong>
                            <p className="mt-1 line-clamp-2 text-sm text-muted-foreground">
                              {entry.description || "无描述"}
                            </p>
                          </div>
                          <Badge variant={entry.status === "需确认" ? "destructive" : "secondary"}>
                            {entry.status}
                          </Badge>
                        </div>
                        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                          <span>{entry.category}</span>
                          <span>使用 {entry.timesUsed} 次</span>
                          <span>更新 {entry.updated || "-"}</span>
                          {entry.tags.slice(0, 4).map((tag) => (
                            <span key={tag} className="rounded bg-muted px-1.5 py-0.5">
                              {tag}
                            </span>
                          ))}
                        </div>
                      </article>
                    ))}
                  </div>
                )}
              </ScrollArea>
            </section>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function BucketChart({
  buckets,
  title,
  total,
}: {
  buckets: MemoryBucket[];
  title: string;
  total: number;
}) {
  const max = Math.max(1, ...buckets.map((bucket) => bucket.count));

  return (
    <div className="grid gap-2 rounded-md border bg-background p-3">
      <h3 className="text-sm font-semibold">{title}</h3>
      <div className="grid gap-2">
        {buckets.map((bucket) => (
          <div key={bucket.label} className="grid gap-1">
            <div className="flex items-center justify-between gap-3 text-xs">
              <span className="text-muted-foreground">{bucket.label}</span>
              <span className="font-medium">{bucket.count}</span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-primary"
                style={{ width: `${total === 0 ? 0 : Math.max(6, (bucket.count / max) * 100)}%` }}
              />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
