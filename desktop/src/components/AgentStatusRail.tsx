import {
  Brain,
  CheckCircle2,
  Circle,
  GitFork,
  MessageSquare,
  Play,
  Plus,
  Trash2,
  Wrench,
} from "lucide-react";
import { ReactNode } from "react";
import { AgentProfile } from "@/agentModel";
import { ToolActivity } from "@/chatState";
import { Button } from "@/components/ui/button";
import { ConversationSession } from "@/conversationSession";
import { cn } from "@/lib/utils";

interface AgentStatusRailProps {
  agent: AgentProfile;
  activeSessionId: string;
  sessions: ConversationSession[];
  onDeleteSession: (sessionId: string) => void;
  onNewSession: () => void;
  onSelectSession: (sessionId: string) => void;
  running: boolean;
  status: string;
  turnCount: number;
  tools: ToolActivity[];
}

export function AgentStatusRail({
  agent,
  activeSessionId,
  sessions,
  onDeleteSession,
  onNewSession,
  onSelectSession,
  running,
  status,
  tools,
  turnCount,
}: AgentStatusRailProps) {
  const phase = resolvePhase(status, tools);
  const activeTool =
    tools.find((tool) => tool.status === "running") ?? tools[tools.length - 1];
  const subagents = deriveRuntimeSubagents(tools);

  return (
    <aside className="hidden min-h-0 border-r bg-muted/40 p-3 min-[1180px]:block">
      <div className="sticky top-3 grid max-h-[calc(100vh-1.5rem)] min-h-0 grid-rows-[auto_minmax(0,1fr)_auto_auto] overflow-hidden rounded-md border bg-background shadow-sm">
        <section className="border-b p-3">
          <div className="flex items-center gap-2">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted text-muted-foreground">
              {phase.icon}
            </span>
            <div className="min-w-0">
              <p className="text-sm font-semibold text-foreground">Agent</p>
              <p className="truncate text-xs text-muted-foreground">{phase.label}</p>
            </div>
          </div>

          <div className="mt-3">
            <div className="mb-1.5 flex items-center justify-between text-xs text-muted-foreground">
              <span>会话脉冲</span>
              <span>{running ? "运行中" : "稳定"}</span>
            </div>
            <div className={cn("agent-pulse", running && "agent-pulse-active")} aria-hidden="true" />
          </div>
        </section>

        <section className="min-h-0 border-b p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <p className="text-xs font-semibold text-muted-foreground">对话</p>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-7"
              aria-label="新建对话"
              onClick={onNewSession}
            >
              <Plus className="size-3.5" aria-hidden="true" />
            </Button>
          </div>
          <div className="no-scrollbar grid max-h-64 gap-1.5 overflow-y-auto pr-1">
            {sessions.map((session) => {
              const messageCount = session.state.messages.filter(
                (message) => message.role === "user",
              ).length;
              return (
                <div
                  key={session.id}
                  className={cn(
                    "group flex min-w-0 items-center gap-2 rounded-md border px-2 py-1.5 text-left text-xs transition-colors hover:border-ring hover:bg-accent/60",
                    session.id === activeSessionId
                      ? "border-primary/35 bg-primary/10 text-foreground"
                      : "bg-background text-muted-foreground",
                  )}
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-2 rounded-[calc(var(--radius)-3px)] text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onClick={() => onSelectSession(session.id)}
                  >
                    <span className="flex size-6 shrink-0 items-center justify-center rounded-md border bg-background">
                      <MessageSquare className="size-3.5" aria-hidden="true" />
                    </span>
                    <span className="min-w-0 flex-1">
                      <strong className="block truncate font-semibold" title={session.title}>
                        {session.title}
                      </strong>
                      <span className="block truncate">
                        {messageCount > 0 ? `${messageCount} 轮` : "未开始"}
                      </span>
                    </span>
                  </button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-7 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
                    aria-label={`删除对话：${session.title}`}
                    onClick={() => onDeleteSession(session.id)}
                    disabled={session.state.running}
                    title={session.state.running ? "运行中的对话不能删除" : "删除对话"}
                  >
                    <Trash2 className="size-3.5" aria-hidden="true" />
                  </Button>
                </div>
              );
            })}
          </div>
        </section>

        <section className="border-b p-3">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs font-semibold text-muted-foreground">Agent 运行</p>
          </div>
          <div className="mt-2 min-w-0">
            <p className="truncate text-sm font-semibold text-foreground" title={agent.name}>
              {agent.name}
            </p>
            <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground" title={agent.role}>
              {agent.role}
            </p>
          </div>
          <div className="mt-3 grid gap-1.5">
            {subagents.length === 0 ? (
              <div className="rounded-md border border-dashed bg-background px-2.5 py-2 text-xs leading-5 text-muted-foreground">
                当前还没有运行时 subagent。模型调用 Subagent 工具或 fork 模式后会显示在这里。
              </div>
            ) : (
              subagents.map((item) => (
                <div
                  key={item.id}
                  className={cn(
                    "flex min-w-0 items-center gap-2 rounded-md border px-2.5 py-2 text-left text-xs",
                    item.status === "running"
                      ? "border-primary/35 bg-primary/10 text-foreground"
                      : "bg-background text-muted-foreground",
                  )}
                >
                <span
                  className={cn(
                    "flex size-6 shrink-0 items-center justify-center rounded-md border bg-background",
                    item.status === "running" && "text-primary",
                  )}
                >
                  <GitFork className="size-3.5" aria-hidden="true" />
                </span>
                <span className="min-w-0 flex-1">
                  <strong className="block truncate font-semibold" title={item.name}>
                    {item.name}
                  </strong>
                  <span className="block truncate" title={item.detail}>
                    {subagentStatusLabel(item.status)} · {item.detail || "runtime subagent"}
                  </span>
                </span>
                </div>
              ))
            )}
          </div>
        </section>

        <section className="border-b p-3">
          <p className="text-xs font-semibold text-muted-foreground">当前阶段</p>
          <div className="mt-3 grid gap-2">
            {phaseSteps.map((step) => (
              <div
                key={step.id}
                className={cn(
                  "flex items-center gap-2 text-xs",
                  phase.id === step.id ? "text-foreground" : "text-muted-foreground",
                )}
              >
                <span
                  className={cn(
                    "flex size-5 items-center justify-center rounded-full border",
                    phase.id === step.id && "border-primary bg-primary text-primary-foreground",
                  )}
                >
                  {phase.id === step.id ? (
                    <Circle className="size-2 fill-current" aria-hidden="true" />
                  ) : (
                    <span className="size-1.5 rounded-full bg-current opacity-45" />
                  )}
                </span>
                <span>{step.label}</span>
              </div>
            ))}
          </div>
        </section>

        <section className="p-3">
          <p className="text-xs font-semibold text-muted-foreground">本轮上下文</p>
          <div className="mt-3 grid gap-2 text-xs text-muted-foreground">
            <div className="flex items-center justify-between gap-2">
              <span>对话</span>
              <strong className="font-mono text-foreground">{turnCount}</strong>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span>工具</span>
              <strong className="font-mono text-foreground">{tools.length}</strong>
            </div>
            {activeTool && (
              <div className="mt-1 min-w-0 rounded-md bg-muted px-2 py-1.5">
                <p className="truncate font-medium text-foreground" title={activeTool.name}>
                  {activeTool.name}
                </p>
                <p className="mt-0.5 truncate" title={activeTool.detail}>
                  {activeTool.detail || toolStatusLabel(activeTool.status)}
                </p>
              </div>
            )}
          </div>
        </section>
      </div>

    </aside>
  );
}

const phaseSteps = [
  { id: "ready", label: "待命" },
  { id: "thinking", label: "思考" },
  { id: "tooling", label: "工具" },
  { id: "done", label: "完成" },
] as const;

type PhaseId = (typeof phaseSteps)[number]["id"];

function resolvePhase(status: string, tools: ToolActivity[]): { id: PhaseId; label: string; icon: ReactNode } {
  const runningTool = tools.some((tool) => tool.status === "running");
  if (runningTool || status === "running") {
    return {
      id: "tooling",
      label: "调用工具",
      icon: <Wrench className="size-4 thinking-breathe" aria-hidden="true" />,
    };
  }

  if (status === "thinking") {
    return {
      id: "thinking",
      label: "思考中",
      icon: <Brain className="size-4 thinking-breathe" aria-hidden="true" />,
    };
  }

  if (status === "completed" || status === "tool completed") {
    return {
      id: "done",
      label: "已完成",
      icon: <CheckCircle2 className="size-4" aria-hidden="true" />,
    };
  }

  return {
    id: "ready",
    label: statusLabel(status),
    icon: <Play className="size-4" aria-hidden="true" />,
  };
}

function toolStatusLabel(status: ToolActivity["status"]): string {
  switch (status) {
    case "running":
      return "运行中";
    case "completed":
      return "完成";
    case "failed":
      return "失败";
  }
}

interface RuntimeSubagent {
  id: string;
  name: string;
  detail: string;
  status: ToolActivity["status"];
}

function deriveRuntimeSubagents(tools: ToolActivity[]): RuntimeSubagent[] {
  return tools
    .filter((tool) => isSubagentTool(tool.name))
    .map((tool, index) => ({
      id: tool.id,
      name: runtimeSubagentName(tool, index),
      detail: tool.detail,
      status: tool.status,
    }));
}

function isSubagentTool(name: string): boolean {
  const normalized = name.toLowerCase();
  return normalized.includes("subagent") || normalized.includes("fork");
}

function runtimeSubagentName(tool: ToolActivity, index: number): string {
  const detail = tool.detail.trim();
  if (!detail) {
    return tool.name || `Subagent ${index + 1}`;
  }

  const firstLine = detail.split(/\r?\n/, 1)[0]?.trim();
  if (!firstLine) {
    return tool.name || `Subagent ${index + 1}`;
  }

  return firstLine.length > 48 ? `${firstLine.slice(0, 45)}...` : firstLine;
}

function subagentStatusLabel(status: ToolActivity["status"]): string {
  switch (status) {
    case "running":
      return "运行中";
    case "completed":
      return "已完成";
    case "failed":
      return "失败";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "idle":
      return "就绪";
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
      return status || "就绪";
  }
}
