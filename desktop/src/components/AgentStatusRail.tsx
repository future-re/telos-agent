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
import { ReactNode, useEffect, useState } from "react";
import { AgentProfile, defaultAgent } from "@/agentModel";
import { ToolActivity } from "@/chatState";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ConversationSession } from "@/conversationSession";
import { cn } from "@/lib/utils";

interface AgentStatusRailProps {
  agent: AgentProfile;
  agents: AgentProfile[];
  activeAgentId: string;
  activeSessionId: string;
  sessions: ConversationSession[];
  onForkSubagent: (agent: Pick<AgentProfile, "name" | "role" | "instructions">) => void;
  onDeleteSession: (sessionId: string) => void;
  onNewSession: () => void;
  onSelectAgent: (agentId: string) => void;
  onSelectSession: (sessionId: string) => void;
  running: boolean;
  status: string;
  turnCount: number;
  tools: ToolActivity[];
}

export function AgentStatusRail({
  agent,
  agents,
  activeAgentId,
  activeSessionId,
  sessions,
  onForkSubagent,
  onDeleteSession,
  onNewSession,
  onSelectAgent,
  onSelectSession,
  running,
  status,
  tools,
  turnCount,
}: AgentStatusRailProps) {
  const phase = resolvePhase(status, tools);
  const activeTool =
    tools.find((tool) => tool.status === "running") ?? tools[tools.length - 1];
  const [dialogOpen, setDialogOpen] = useState(false);
  const [draft, setDraft] = useState(agent);

  useEffect(() => {
    if (!dialogOpen) {
      setDraft(createForkDraft(agent));
    }
  }, [agent, dialogOpen]);

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
            <p className="text-xs font-semibold text-muted-foreground">Agent 身份</p>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-7"
              aria-label="Fork subagent"
              onClick={() => {
                setDraft(createForkDraft(agent));
                setDialogOpen(true);
              }}
            >
              <GitFork className="size-3.5" aria-hidden="true" />
            </Button>
          </div>
          <div className="mt-2 min-w-0">
            <p className="truncate text-sm font-semibold text-foreground" title={agent.name}>
              {agent.name}
            </p>
            <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground" title={agent.role}>
              {agent.role}
            </p>
            {agent.kind === "subagent" && (
              <p className="mt-2 inline-flex rounded-full border bg-muted px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
                forked subagent
              </p>
            )}
          </div>
          <div className="mt-3 grid gap-1.5">
            {agents.map((item) => (
              <button
                key={item.id}
                type="button"
                className={cn(
                  "flex min-w-0 items-center gap-2 rounded-md border px-2.5 py-2 text-left text-xs transition-colors hover:border-ring hover:bg-accent/60",
                  item.id === activeAgentId
                    ? "border-primary/35 bg-primary/10 text-foreground"
                    : "bg-background text-muted-foreground",
                )}
                onClick={() => onSelectAgent(item.id)}
              >
                <span
                  className={cn(
                    "flex size-6 shrink-0 items-center justify-center rounded-md border bg-background",
                    item.kind === "subagent" && "text-primary",
                  )}
                >
                  {item.kind === "subagent" ? (
                    <GitFork className="size-3.5" aria-hidden="true" />
                  ) : (
                    <Play className="size-3.5" aria-hidden="true" />
                  )}
                </span>
                <span className="min-w-0 flex-1">
                  <strong className="block truncate font-semibold" title={item.name}>
                    {item.name}
                  </strong>
                  <span className="block truncate" title={item.role}>
                    {item.kind === "subagent" ? "Subagent" : "Primary"} · {item.role}
                  </span>
                </span>
              </button>
            ))}
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

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Fork Subagent</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="rounded-md border bg-muted/35 px-3 py-2 text-xs leading-5 text-muted-foreground">
              这里创建的是当前桌面对话的 Agent 身份。运行时已经注册 Subagent 工具，模型可在需要时调用系统级 subagent 或 fork 模式。
            </div>
            <label className="grid gap-1.5">
              <span className="text-sm font-medium">Subagent 名称</span>
              <Input
                value={draft.name}
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
                placeholder="例如：UI Polish Subagent"
              />
            </label>
            <label className="grid gap-1.5">
              <span className="text-sm font-medium">角色</span>
              <Input
                value={draft.role}
                onChange={(event) => setDraft({ ...draft, role: event.target.value })}
                placeholder="例如：负责 UI、代码修改和验证"
              />
            </label>
            <label className="grid gap-1.5">
              <span className="text-sm font-medium">行为说明</span>
              <Textarea
                value={draft.instructions}
                onChange={(event) => setDraft({ ...draft, instructions: event.target.value })}
                placeholder="定义这个 Agent 的偏好、边界和工作方式"
                className="min-h-28"
              />
            </label>
          </div>
          <DialogFooter>
            <Button
              type="button"
              onClick={() => {
                onForkSubagent({
                  name: draft.name.trim() || defaultAgent.name,
                  role: draft.role.trim() || defaultAgent.role,
                  instructions: draft.instructions.trim(),
                });
                setDialogOpen(false);
              }}
            >
              Fork Subagent
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
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

function createForkDraft(agent: AgentProfile): AgentProfile {
  return {
    ...agent,
    id: "",
    kind: "subagent",
    parentId: agent.id,
    name: `${agent.name} Subagent`,
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
      return status || "就绪";
  }
}
