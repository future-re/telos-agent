import { Activity, Bot, Check, Circle, Folder, KeyRound, Library, Wrench } from "lucide-react";
import { ToolActivity } from "@/chatState";
import { SettingsSection } from "@/desktopTypes";
import { RunDisplay } from "@/runDisplay";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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
    <div className="grid h-full min-w-0 grid-rows-[auto_auto_auto_minmax(0,1fr)] gap-2.5 overflow-hidden bg-muted/40 p-3">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="font-mono text-xs font-bold uppercase text-muted-foreground">运行状态</p>
          <h2 className="mt-1 truncate text-xl font-semibold leading-tight tracking-normal" title={display.activityLabel}>
            {display.activityLabel}
          </h2>
        </div>
        <Badge variant={running ? "success" : "outline"} className="max-w-28 shrink-0 truncate">
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
          <CardHeader className="flex-row items-center space-y-0 p-3 pb-2">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Activity className="size-4" aria-hidden="true" />
              当前模型
            </CardTitle>
          </CardHeader>
          <CardContent className="p-3 pt-0">
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
        <CardHeader className="flex-row items-center justify-between space-y-0 p-3 pb-2">
          <CardTitle className="flex items-center gap-2 text-sm">
            <Activity className="size-4" aria-hidden="true" />
            执行视图
          </CardTitle>
          <span className="font-mono text-xs font-bold uppercase text-muted-foreground">
            {tools.length} 条
          </span>
        </CardHeader>
        <CardContent className="min-h-0 min-w-0 p-3 pt-0">
          <div className="grid gap-3">
            <div className="rounded-md border bg-muted/30 p-3 text-sm text-muted-foreground">
              主对话区现在会直接展示执行步骤和结果摘要。这里保留运行概览，不再作为主要日志视图。
            </div>
            <div className="grid grid-cols-3 gap-2">
              <MetricSummary label="执行中" value={tools.filter((tool) => tool.status === "running").length} />
              <MetricSummary label="完成" value={tools.filter((tool) => tool.status === "completed").length} />
              <MetricSummary label="失败" value={tools.filter((tool) => tool.status === "failed").length} />
            </div>
            {tools.length === 0 ? (
              <div className="flex items-center gap-2 rounded-md border border-dashed bg-muted/40 p-3 text-sm text-muted-foreground">
                <Wrench className="size-4" aria-hidden="true" />
                当前还没有工具步骤。
              </div>
            ) : null}
          </div>
        </CardContent>
      </Card>
    </div>
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
    <CardContent className="p-2.5 text-left">
      <span className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        {icon}
        {label}
      </span>
      <strong className="mt-1.5 block truncate text-sm" title={value}>
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

function MetricSummary({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md border bg-background px-3 py-2">
      <div className="text-xs text-muted-foreground">{label}</div>
      <strong className="mt-1 block font-mono text-lg text-foreground">{value}</strong>
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
      return "执行失败";
    default:
      return status || "空闲";
  }
}
