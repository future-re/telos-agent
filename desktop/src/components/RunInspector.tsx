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
import { SettingsSection } from "@/desktopTypes";
import { RunDisplay } from "@/runDisplay";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

interface RunInspectorProps {
  display: RunDisplay;
  onChooseDirectory: () => void;
  onConfigure: (section: SettingsSection) => void;
  onOpenMemory: () => void;
  running: boolean;
  status: string;
  tools: ToolActivity[];
}

export function RunInspector({
  display,
  onChooseDirectory,
  onConfigure,
  onOpenMemory,
  running,
  status,
  tools,
}: RunInspectorProps) {
  return (
    <aside className="grid h-screen min-w-0 grid-rows-[auto_auto_auto_minmax(0,1fr)] gap-3 overflow-hidden border-l bg-muted/40 p-4 max-[920px]:h-auto max-[920px]:min-h-0 max-[920px]:border-l-0 max-[920px]:border-t">
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
        <Metric
          actionLabel="打开服务设置"
          icon={<Bot className="size-4" />}
          label="服务"
          onClick={() => onConfigure("service")}
          value={display.providerLabel}
        />
        <Metric
          actionLabel="打开密钥设置"
          icon={<KeyRound className="size-4" />}
          label="密钥"
          onClick={() => onConfigure("key")}
          value={display.apiKeyLabel}
        />
        <Metric
          actionLabel="打开权限设置"
          icon={<Check className="size-4" />}
          label="权限"
          onClick={() => onConfigure("approval")}
          value={display.approvalLabel}
        />
        <Metric
          actionLabel="查看记忆"
          icon={<Library className="size-4" />}
          label="记忆"
          onClick={onOpenMemory}
          value={display.memoryLabel}
        />
        <Metric
          actionLabel="选择目录"
          className="col-span-2"
          icon={<Folder className="size-4" />}
          label="目录"
          onClick={onChooseDirectory}
          value={display.workspaceLabel}
        />
      </section>

      <Card className="transition-colors hover:border-ring hover:bg-accent/60">
        <button
          type="button"
          className="block w-full text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => onConfigure("model")}
          aria-label="打开模型设置"
        >
          <CardHeader className="flex-row items-center space-y-0 pb-3">
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
                {display.modelDescription}
              </span>
            </div>
          </CardContent>
        </button>
      </Card>

      <Card className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
        <CardHeader className="flex-row items-center justify-between space-y-0 pb-3">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Activity className="size-4" aria-hidden="true" />
            工具活动
          </CardTitle>
          <span className="font-mono text-xs font-bold uppercase text-muted-foreground">
            {tools.length} 条
          </span>
        </CardHeader>
        <CardContent className="min-h-0 min-w-0">
          <ScrollArea className="h-full min-w-0">
            {tools.length === 0 ? (
              <div className="flex items-center gap-2 rounded-md border border-dashed bg-muted/40 p-3 text-sm text-muted-foreground">
                <Wrench className="size-4" aria-hidden="true" />
                运行任务时，工具调用会显示在这里。
              </div>
            ) : (
              <div className="grid min-w-0 gap-2">
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
  actionLabel,
  className,
  icon,
  label,
  onClick,
  value,
}: {
  actionLabel?: string;
  className?: string;
  icon: React.ReactNode;
  label: string;
  onClick?: () => void;
  value: string;
}) {
  const content = (
    <CardContent className="p-3 text-left">
      <span className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        {icon}
        {label}
      </span>
      <strong className="mt-2 block truncate text-sm" title={value}>
        {value}
      </strong>
    </CardContent>
  );

  if (onClick) {
    return (
      <Card
        className={cn(
          "min-w-0 transition-colors hover:border-ring hover:bg-accent/60",
          className,
        )}
      >
        <button
          type="button"
          className="block h-full w-full text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label={actionLabel ?? label}
          onClick={onClick}
        >
          {content}
        </button>
      </Card>
    );
  }

  return (
    <Card className={cn("min-w-0", className)}>
      {content}
    </Card>
  );
}

function ToolItem({ tool }: { tool: ToolActivity }) {
  return (
    <div
      className={cn(
        "flex min-h-14 min-w-0 items-center gap-2 overflow-hidden rounded-md border bg-background p-2",
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
      <div className="min-w-0 flex-1 overflow-hidden">
        <strong className="block min-w-0 truncate text-sm" title={tool.name}>
          {tool.name}
        </strong>
        <span className="block min-w-0 truncate text-xs text-muted-foreground" title={tool.detail || statusLabel(tool.status)}>
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
