import { useEffect, useMemo, useRef } from "react";
import {
  Check,
  Clock3,
  FilePenLine,
  FileSearch,
  FileText,
  KeyRound,
  Sparkles,
  TerminalSquare,
  XCircle,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ChatMessage, TokenUsage } from "@/chatState";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ConversationTurn, groupConversationMessages } from "@/conversationView";
import { cn } from "@/lib/utils";
import { estimateCost, formatCost, formatTokenCount } from "@/tokenUsage";

interface ConversationProps {
  messages: ChatMessage[];
  needsApiKey: boolean;
  usageByTurnId: Record<string, TokenUsage>;
  apiKeyDraft: string;
  savingKey: boolean;
  onApiKeyChange: (value: string) => void;
  onConfigureApiKey: () => void;
  onPickPrompt: (prompt: string) => void;
}

const roleLabels: Record<ConversationTurn["role"], string> = {
  user: "你",
  assistant: "telos",
  system: "系统",
  tool: "执行",
};

export function Conversation({
  apiKeyDraft,
  messages,
  needsApiKey,
  usageByTurnId,
  onApiKeyChange,
  onConfigureApiKey,
  onPickPrompt,
  savingKey,
}: ConversationProps) {
  const endRef = useRef<HTMLDivElement | null>(null);
  const turns = useMemo(() => groupConversationMessages(messages), [messages]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [turns]);

  return (
    <section className="h-full min-h-0 overflow-hidden bg-[radial-gradient(circle_at_top_left,rgba(37,99,235,0.08),transparent_32rem),linear-gradient(180deg,var(--background),var(--muted))]">
      <ScrollArea className="h-full w-full">
        <div className="mx-auto flex min-h-full w-full min-w-0 max-w-4xl flex-col px-4 py-6 md:px-6 md:py-8">
          {turns.length === 0 ? (
            <div className="flex min-h-[420px] flex-1 items-end pb-8 md:pb-12">
              <div className="w-full min-w-0">
                {needsApiKey ? (
                  <OnboardingCard
                    apiKeyDraft={apiKeyDraft}
                    onApiKeyChange={onApiKeyChange}
                    onConfigureApiKey={onConfigureApiKey}
                    savingKey={savingKey}
                  />
                ) : (
                  <PromptStarter onPickPrompt={onPickPrompt} />
                )}
              </div>
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col gap-6">
              {turns.map((turn) => (
                <MessageTurn
                  key={turn.id}
                  turn={turn}
                  usage={turn.turnId ? usageByTurnId[turn.turnId] : undefined}
                />
              ))}
              <div ref={endRef} />
            </div>
          )}
        </div>
      </ScrollArea>
    </section>
  );
}

function MessageTurn({ turn, usage }: { turn: ConversationTurn; usage?: TokenUsage }) {
  if (turn.role === "system") {
    return (
      <div className="flex min-w-0 justify-center">
        <p className="max-w-2xl rounded-full border border-border bg-secondary/60 px-3 py-1.5 text-xs leading-5 text-muted-foreground">
          {turn.content}
        </p>
      </div>
    );
  }

  if (turn.role === "user") {
    return (
      <article className="flex min-w-0 justify-end">
        <div className="min-w-0 max-w-[min(680px,82%)]">
          <div className="mb-1.5 pr-1 text-right text-[13px] font-medium text-muted-foreground">
            {roleLabels.user}
          </div>
          <div className="min-w-0 overflow-hidden rounded-2xl rounded-br-md bg-primary px-4 py-3 text-[15px] leading-7 text-primary-foreground shadow-[0_10px_24px_rgba(15,23,42,0.14)]">
            <MarkdownContent className="markdown-body markdown-body-user" content={turn.content} />
          </div>
        </div>
      </article>
    );
  }

  if (turn.role === "tool") {
    return (
      <article className="min-w-0">
        <div
          className={cn(
            "rounded-xl border bg-background/90 px-4 py-3 shadow-sm",
            turn.toolStatus === "completed" && "border-emerald-200",
            turn.toolStatus === "failed" && "border-red-200",
          )}
        >
          <div className="mb-2 flex items-center gap-2 text-[13px] font-medium text-muted-foreground">
            <span className="flex size-7 items-center justify-center rounded-md border bg-muted/30">
              {turn.toolStatus === "completed" ? (
                <Check className="size-4 text-emerald-600" aria-hidden="true" />
              ) : turn.toolStatus === "failed" ? (
                <XCircle className="size-4 text-red-600" aria-hidden="true" />
              ) : (
                <Clock3 className="size-4 text-muted-foreground" aria-hidden="true" />
              )}
            </span>
            <span className="flex items-center gap-2">
              <TerminalSquare className="size-4" aria-hidden="true" />
              {turn.toolName ?? "工具执行"}
            </span>
            <span
              className={cn(
                "rounded-full border px-2 py-0.5 text-[11px]",
                turn.toolStatus === "completed" &&
                  "border-emerald-200 bg-emerald-50 text-emerald-700",
                turn.toolStatus === "failed" && "border-red-200 bg-red-50 text-red-700",
                turn.toolStatus === "running" && "border-slate-200 bg-slate-50 text-slate-700",
              )}
            >
              {turn.toolStatus === "completed"
                ? "完成"
                : turn.toolStatus === "failed"
                  ? "失败"
                  : "执行中"}
            </span>
            {turn.streaming ? <span className="thinking-dots" aria-label="执行中" /> : null}
          </div>
          <ToolMessageBody turn={turn} />
        </div>
      </article>
    );
  }

  const assistantTurn = turn;

  return (
    <article className="min-w-0">
      <div className="min-w-0 border-l pl-4">
        <div className="mb-2 flex items-center gap-2 text-[13px] font-medium text-muted-foreground">
          <span>{roleLabels.assistant}</span>
          {assistantTurn.streaming && <span className="thinking-dots" aria-label="正在生成" />}
          {usage && (
            <span className="inline-flex items-center gap-2 rounded-full border bg-background px-2 py-0.5 font-mono text-[11px]">
              <span>{formatTokenCount(usage.totalTokens)} tokens</span>
              {usage.promptCacheHitTokens !== undefined && (
                <span className="text-green-600">
                  hit {formatTokenCount(usage.promptCacheHitTokens)}
                </span>
              )}
              {usage.promptCacheMissTokens !== undefined && (
                <span className="text-amber-600">
                  miss {formatTokenCount(usage.promptCacheMissTokens)}
                </span>
              )}
              {(() => {
                const cost = estimateCost(usage.model, usage);
                return cost && cost.totalCost > 0 ? (
                  <span className="text-muted-foreground">{formatCost(cost.totalCost)}</span>
                ) : null;
              })()}
            </span>
          )}
        </div>
        {assistantTurn.thinking && (
          <details className="group mb-3 text-[13px] text-muted-foreground">
            <summary className="inline-flex cursor-pointer select-none items-center gap-2 rounded-md px-0 font-medium text-muted-foreground hover:text-foreground">
              <span className="thinking-spark" aria-hidden="true" />
              思考过程
            </summary>
            <div className="thinking-panel mt-2 whitespace-pre-wrap break-words pl-3 leading-6 text-muted-foreground">
              {assistantTurn.thinking}
            </div>
          </details>
        )}
        <MarkdownContent
          className="markdown-body text-[15px] leading-8 text-foreground"
          content={assistantTurn.content || (assistantTurn.streaming ? "正在生成..." : "")}
        />
      </div>
    </article>
  );
}

function ToolMessageBody({ turn }: { turn: Extract<ConversationTurn, { role: "tool" }> }) {
  const view = buildToolMessageView(turn);

  if (!view) {
    return (
      <MarkdownContent
        className="markdown-body text-[14px] leading-7 text-foreground"
        content={turn.content}
      />
    );
  }

  return (
    <div className="grid gap-3">
      <div className="flex min-w-0 items-start gap-3 rounded-lg border bg-muted/20 px-3 py-3">
        <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground">
          <view.icon className="size-4" aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <strong className="block text-[14px] leading-6 text-foreground">{view.title}</strong>
          {view.subtitle ? (
            <p className="mt-0.5 break-words text-xs leading-5 text-muted-foreground">
              {view.subtitle}
            </p>
          ) : null}
        </div>
      </div>

      {view.command ? <ToolCodeBlock label="命令" content={view.command} /> : null}

      {view.paths.length > 0 ? (
        <div className="grid gap-2">
          {view.paths.map((path) => (
            <div
              key={path}
              className="rounded-md border bg-background px-3 py-2 font-mono text-xs leading-5 text-foreground"
            >
              {path}
            </div>
          ))}
        </div>
      ) : null}

      {view.output ? <ToolCodeBlock label={view.outputLabel} content={view.output} /> : null}

      {view.notes.length > 0 ? (
        <div className="grid gap-2">
          {view.notes.map((note, index) => (
            <div
              key={`${note}-${index}`}
              className="rounded-md border bg-background px-3 py-2 text-sm leading-6 text-foreground"
            >
              {note}
            </div>
          ))}
        </div>
      ) : null}

      {view.fallback && !view.output ? (
        <MarkdownContent
          className="markdown-body text-[14px] leading-7 text-foreground"
          content={view.fallback}
        />
      ) : null}
    </div>
  );
}

function ToolCodeBlock({ label, content }: { label: string; content: string }) {
  return (
    <div className="grid gap-1.5">
      <span className="text-[11px] font-medium uppercase text-muted-foreground">{label}</span>
      <pre className="max-h-80 overflow-auto rounded-lg border bg-slate-950 px-3 py-3 font-mono text-xs leading-5 text-slate-50">
        {content}
      </pre>
    </div>
  );
}

type ToolMessageView = {
  icon: typeof TerminalSquare;
  title: string;
  subtitle?: string;
  command?: string;
  paths: string[];
  output?: string;
  outputLabel: string;
  notes: string[];
  fallback?: string;
};

function buildToolMessageView(
  turn: Extract<ConversationTurn, { role: "tool" }>,
): ToolMessageView | undefined {
  const toolName = (turn.toolName ?? "").trim();
  const detail = (turn.toolDetail ?? "").trim();
  const result = asObject(turn.toolResultContent);
  const normalizedName = toolName.toLowerCase();

  if (normalizedName === "bash" || normalizedName === "powershell") {
    const stdout = stringField(result, "stdout");
    const stderr = stringField(result, "stderr");
    const command = detail || undefined;
    const title = command ? `Ran ${command}` : `Ran ${toolName || "command"}`;
    const output = [stdout, stderr].filter((item) => item && item.trim()).join("\n\n");
    return {
      icon: TerminalSquare,
      title,
      subtitle: command ? undefined : detail || undefined,
      command,
      paths: [],
      output: output || (turn.toolStatus === "running" ? turn.content : undefined),
      outputLabel: stderr && !stdout ? "错误输出" : "输出",
      notes: collectToolNotes(result),
      fallback: turn.content,
    };
  }

  if (normalizedName === "edit") {
    const path = stringField(result, "file_path") ?? detail;
    return {
      icon: FilePenLine,
      title: path ? `Edited ${path}` : "Edited file",
      subtitle: boolField(result, "replace_all") ? "Applied replace_all" : undefined,
      command: undefined,
      paths: path ? [path] : [],
      output: undefined,
      outputLabel: "结果",
      notes: collectToolNotes(result),
      fallback: turn.content,
    };
  }

  if (normalizedName === "write") {
    const path = stringField(result, "file_path") ?? detail;
    return {
      icon: FileText,
      title: path ? `Wrote ${path}` : "Wrote file",
      subtitle: undefined,
      command: undefined,
      paths: path ? [path] : [],
      output: undefined,
      outputLabel: "结果",
      notes: collectToolNotes(result),
      fallback: turn.content,
    };
  }

  if (normalizedName === "read") {
    const path = stringField(result, "file_path") ?? detail;
    return {
      icon: FileSearch,
      title: path ? `Read ${path}` : "Read file",
      subtitle: lineRangeSummary(result),
      command: undefined,
      paths: path ? [path] : [],
      output: stringField(result, "content") ?? turn.content,
      outputLabel: "文件内容",
      notes: collectToolNotes(result),
      fallback: turn.content,
    };
  }

  return undefined;
}

function asObject(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function stringField(
  value: Record<string, unknown> | undefined,
  key: string,
): string | undefined {
  return typeof value?.[key] === "string" ? (value[key] as string) : undefined;
}

function boolField(value: Record<string, unknown> | undefined, key: string): boolean {
  return value?.[key] === true;
}

function numberField(
  value: Record<string, unknown> | undefined,
  key: string,
): number | undefined {
  return typeof value?.[key] === "number" ? (value[key] as number) : undefined;
}

function lineRangeSummary(value: Record<string, unknown> | undefined): string | undefined {
  const startLine = numberField(value, "start_line");
  const totalLines = numberField(value, "total_lines");
  if (startLine === undefined && totalLines === undefined) {
    return undefined;
  }
  if (startLine !== undefined && totalLines !== undefined) {
    return `From line ${startLine} · ${totalLines} lines total`;
  }
  if (startLine !== undefined) {
    return `From line ${startLine}`;
  }
  return `${totalLines} lines total`;
}

function collectToolNotes(value: Record<string, unknown> | undefined): string[] {
  if (!value) {
    return [];
  }

  const skip = new Set([
    "stdout",
    "stderr",
    "content",
    "file_path",
    "path",
    "start_line",
    "total_lines",
    "replace_all",
  ]);

  return Object.entries(value)
    .filter(([key]) => !skip.has(key))
    .flatMap(([key, entryValue]) => {
      if (
        typeof entryValue === "string" ||
        typeof entryValue === "number" ||
        typeof entryValue === "boolean"
      ) {
        return [`${key}: ${String(entryValue)}`];
      }
      return [];
    });
}

function MarkdownContent({ className, content }: { className?: string; content: string }) {
  return (
    <div className={cn("min-w-0", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ children, href }) => (
            <a href={href} rel="noreferrer" target="_blank">
              {children}
            </a>
          ),
          table: ({ children }) => (
            <div className="markdown-table-wrap">
              <table>{children}</table>
            </div>
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

function OnboardingCard({
  apiKeyDraft,
  onApiKeyChange,
  onConfigureApiKey,
  savingKey,
}: {
  apiKeyDraft: string;
  onApiKeyChange: (value: string) => void;
  onConfigureApiKey: () => void;
  savingKey: boolean;
}) {
  return (
    <div className="mx-auto grid w-full max-w-xl gap-5">
      <div>
        <p className="mb-2 text-xs font-medium text-muted-foreground">首次设置</p>
        <h2 className="text-2xl font-semibold leading-tight tracking-normal text-foreground">
          配置 DeepSeek API Key
        </h2>
        <p className="mt-3 text-sm leading-6 text-muted-foreground">
          密钥会写入 CLI 共用的用户配置文件。配置完成后，项目配置、记忆和工作目录会按 CLI 规则生效。
        </p>
      </div>
      <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
        <div className="relative min-w-0">
          <KeyRound className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-10 pl-9"
            type="password"
            value={apiKeyDraft}
            onChange={(event) => onApiKeyChange(event.target.value)}
            placeholder="DeepSeek API Key"
          />
        </div>
        <Button
          type="button"
          onClick={onConfigureApiKey}
          disabled={savingKey || !apiKeyDraft.trim()}
        >
          保存并开始
        </Button>
      </div>
    </div>
  );
}

function PromptStarter({ onPickPrompt }: { onPickPrompt: (prompt: string) => void }) {
  return (
    <div className="mx-auto grid w-full max-w-2xl justify-items-center gap-4 text-center">
      <div className="flex size-11 items-center justify-center rounded-xl border bg-background/75 text-muted-foreground shadow-sm">
        <Sparkles className="size-5" aria-hidden="true" />
      </div>
      <div className="grid gap-2">
        <h2 className="text-3xl font-semibold leading-tight tracking-normal text-foreground">
          给 telos 一个明确任务
        </h2>
        <p className="text-[15px] leading-7 text-muted-foreground">
          可以检查当前工作区、解释变更、修改代码或执行验证。
        </p>
      </div>
      <div className="flex flex-wrap justify-center gap-2">
        <Button type="button" variant="outline" onClick={() => onPickPrompt("检查这个仓库")}>
          检查这个仓库
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => onPickPrompt("解释当前 desktop 变更")}
        >
          解释 desktop 变更
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => onPickPrompt("运行相关桌面端测试")}
        >
          运行桌面端测试
        </Button>
      </div>
    </div>
  );
}
