import {
  Brain,
  CheckCircle2,
  MessageSquare,
  Play,
  Plus,
  Trash2,
  Wrench,
} from "lucide-react";
import { ReactNode } from "react";
import { ToolActivity } from "@/chatState";
import { Button } from "@/components/ui/button";
import { ConversationSession } from "@/conversationSession";
import { cn } from "@/lib/utils";

interface AgentStatusRailProps {
  activeSessionId: string;
  sessions: ConversationSession[];
  onDeleteSession: (sessionId: string) => void;
  onNewSession: () => void;
  onSelectSession: (sessionId: string) => void;
  running: boolean;
  status: string;
  tools: ToolActivity[];
}

export function AgentStatusRail({
  activeSessionId,
  sessions,
  onDeleteSession,
  onNewSession,
  onSelectSession,
  running,
  status,
  tools,
}: AgentStatusRailProps) {
  const phase = resolvePhase(status, tools);

  return (
    <aside className="hidden h-full min-h-0 min-w-0 overflow-hidden border-r bg-muted p-3 min-[1180px]:block">
      <div className="grid h-full min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-md border bg-background shadow-sm">
        <section className="border-b p-3">
          <div className="flex items-center gap-2">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted text-muted-foreground">
              {phase.icon}
            </span>
            <div className="min-w-0">
              <p className="text-sm font-semibold text-foreground">Agent</p>
              <p className="truncate text-xs text-muted-foreground">
                {phase.label}
              </p>
            </div>
          </div>

          <div className="mt-3 rounded-md border bg-muted/30 p-2.5">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-[0.68rem] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
                Runtime
              </span>
              <span
                className="min-w-0 max-w-24 truncate rounded-md border bg-background px-1.5 py-0.5 text-[0.68rem] text-muted-foreground"
                title={phase.label}
              >
                {phase.label}
              </span>
            </div>
            <div className="grid grid-cols-4 gap-1" aria-label="当前阶段">
              {phaseSteps.map((step) => (
                <div key={step.id} className="min-w-0">
                  <div
                    className={cn(
                      "h-1 rounded-full bg-border",
                      phase.id === step.id && "bg-primary",
                    )}
                  />
                  <p
                    className={cn(
                      "mt-1 truncate text-center text-[0.68rem]",
                      phase.id === step.id
                        ? "font-medium text-foreground"
                        : "text-muted-foreground",
                    )}
                    title={step.label}
                  >
                    {step.label}
                  </p>
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden p-3">
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
          <div className="flex h-full min-h-0 min-w-0 flex-col gap-1.5 overflow-x-hidden overflow-y-auto pr-1">
            {sessions.map((session) => {
              const messageCount = session.state.messages.filter(
                (message) => message.role === "user",
              ).length;
              return (
                <div
                  key={session.id}
                  className={cn(
                    "group flex w-full min-w-0 max-w-full shrink-0 items-center gap-2 overflow-hidden rounded-md border px-2 py-1.5 text-left text-xs transition-colors hover:border-ring hover:bg-accent/60",
                    session.id === activeSessionId
                      ? "border-primary/35 bg-primary/10 text-foreground"
                      : "bg-background text-muted-foreground",
                  )}
                >
                  <button
                    type="button"
                    className="flex min-w-0 flex-1 basis-0 items-center gap-2 overflow-hidden rounded-[calc(var(--radius)-3px)] text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onClick={() => onSelectSession(session.id)}
                  >
                    <span className="flex size-6 shrink-0 items-center justify-center rounded-md border bg-background">
                      <MessageSquare className="size-3.5" aria-hidden="true" />
                    </span>
                    <span className="min-w-0 flex-1 basis-0 overflow-hidden">
                      <strong
                        className="block truncate font-semibold"
                        title={session.title}
                      >
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
                    title={
                      session.state.running
                        ? "运行中的对话不能删除"
                        : "删除对话"
                    }
                  >
                    <Trash2 className="size-3.5" aria-hidden="true" />
                  </Button>
                </div>
              );
            })}
          </div>
        </section>
      </div>
    </aside>
  );
}

type PhaseId = "ready" | "thinking" | "tooling" | "done";

const phaseSteps: Array<{ id: PhaseId; label: string }> = [
  { id: "ready", label: "待命" },
  { id: "thinking", label: "思考" },
  { id: "tooling", label: "工具" },
  { id: "done", label: "完成" },
];

function resolvePhase(
  status: string,
  tools: ToolActivity[],
): { id: PhaseId; label: string; icon: ReactNode } {
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
      if (status.startsWith("provider error")) return "接口错误";
      if (status.startsWith("error")) return "错误";
      return status.length > 48 ? `${status.slice(0, 45)}…` : status || "就绪";
  }
}
