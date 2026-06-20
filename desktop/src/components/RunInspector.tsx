import {
  Activity,
  Bot,
  Check,
  Circle,
  Folder,
  KeyRound,
  Library,
  Play,
  Wrench,
  XCircle,
} from "lucide-react";
import { ToolActivity } from "@/chatState";
import { RunDisplay } from "@/runDisplay";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

interface RunInspectorProps {
  display: RunDisplay;
  running: boolean;
  status: string;
  tools: ToolActivity[];
}

export function RunInspector({ display, running, status, tools }: RunInspectorProps) {
  return (
    <aside className="grid min-h-screen min-w-0 grid-rows-[auto_auto_auto_minmax(0,1fr)] gap-3 border-l bg-muted/40 p-4 max-[920px]:min-h-0 max-[920px]:border-l-0 max-[920px]:border-t">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="font-mono text-xs font-bold uppercase text-muted-foreground">运行状态</p>
          <h2 className="mt-1 text-2xl font-semibold leading-none tracking-normal">
            {display.activityLabel}
          </h2>
        </div>
        <Badge variant={running ? "success" : "outline"} className="max-w-40 truncate">
          <Circle className="size-2" aria-hidden="true" />
          {statusLabel(status)}
        </Badge>
      </div>

      <section className="grid grid-cols-2 gap-2" aria-label="当前配置">
        <Metric icon={<Bot className="size-4" />} label="Provider" value={display.providerLabel} />
        <Metric icon={<KeyRound className="size-4" />} label="API Key" value={display.apiKeyLabel} />
        <Metric icon={<Check className="size-4" />} label="工具批准" value={display.approvalLabel} />
        <Metric icon={<Library className="size-4" />} label="记忆" value={display.memoryLabel} />
        <Metric
          className="col-span-2"
          icon={<Folder className="size-4" />}
          label="项目根目录"
          value={display.projectLabel}
        />
        <Metric
          className="col-span-2"
          icon={<Folder className="size-4" />}
          label="工作目录"
          value={display.cwdLabel}
        />
      </section>

      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Activity className="size-4" aria-hidden="true" />
            当前模型
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="rounded-md border bg-background px-3 py-2">
            <strong className="block truncate text-sm" title={display.modelLabel}>
              {display.modelLabel}
            </strong>
            <span className="mt-1 block text-xs text-muted-foreground">
              {display.runMetadata}
            </span>
          </div>
        </CardContent>
      </Card>

      <Card className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)]">
        <CardHeader className="flex-row items-center justify-between space-y-0 pb-3">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Activity className="size-4" aria-hidden="true" />
            工具活动
          </CardTitle>
          <span className="font-mono text-xs font-bold uppercase text-muted-foreground">
            {tools.length} 条
          </span>
        </CardHeader>
        <CardContent className="min-h-0">
          <ScrollArea className="h-full">
            {tools.length === 0 ? (
              <div className="flex items-center gap-2 rounded-md border border-dashed bg-muted/40 p-3 text-sm text-muted-foreground">
                <Wrench className="size-4" aria-hidden="true" />
                运行任务时，工具调用会显示在这里。
              </div>
            ) : (
              <div className="grid gap-2">
                {tools.map((tool) => (
                  <ToolItem key={tool.id} tool={tool} />
                ))}
              </div>
            )}
          </ScrollArea>
        </CardContent>
      </Card>
    </aside>
  );
}

function Metric({
  className,
  icon,
  label,
  value,
}: {
  className?: string;
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <Card className={cn("min-w-0", className)}>
      <CardContent className="p-3">
        <span className="flex items-center gap-1.5 font-mono text-xs font-bold uppercase text-muted-foreground">
          {icon}
          {label}
        </span>
        <strong className="mt-2 block truncate text-sm" title={value}>
          {value}
        </strong>
      </CardContent>
    </Card>
  );
}

function ToolItem({ tool }: { tool: ToolActivity }) {
  return (
    <div
      className={cn(
        "flex min-h-14 items-center gap-2 rounded-md border bg-background p-2",
        tool.status === "failed" && "border-red-200 bg-red-50",
        tool.status === "completed" && "border-emerald-200 bg-emerald-50",
      )}
    >
      <span className="flex size-7 shrink-0 items-center justify-center rounded-md border bg-background">
        {tool.status === "failed" ? (
          <XCircle className="size-4 text-red-600" aria-hidden="true" />
        ) : tool.status === "completed" ? (
          <Check className="size-4 text-emerald-700" aria-hidden="true" />
        ) : (
          <Play className="size-4 text-muted-foreground" aria-hidden="true" />
        )}
      </span>
      <div className="min-w-0">
        <strong className="block truncate text-sm">{tool.name}</strong>
        <span className="block truncate text-xs text-muted-foreground">
          {tool.detail || statusLabel(tool.status)}
        </span>
      </div>
      <em className="ml-auto shrink-0 text-xs not-italic text-muted-foreground">
        {statusLabel(tool.status)}
      </em>
    </div>
  );
}

function statusLabel(status: string): string {
  switch (status) {
    case "idle":
      return "空闲";
    case "thinking":
      return "思考中";
    case "running":
      return "运行中";
    case "completed":
    case "tool completed":
      return "完成";
    case "failed":
    case "tool failed":
      return "失败";
    default:
      return status || "空闲";
  }
}
