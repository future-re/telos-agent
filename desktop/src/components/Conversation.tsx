import { useEffect, useMemo, useRef } from "react";
import { KeyRound, Sparkles } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { ChatMessage, TokenUsage } from "@/chatState";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ConversationTurn, groupConversationMessages } from "@/conversationView";
import { cn } from "@/lib/utils";
import { formatTokenCount } from "@/tokenUsage";

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
        <p className="max-w-2xl rounded-full border border-amber-200 bg-amber-50 px-3 py-1.5 text-xs leading-5 text-amber-950">
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

  const assistantTurn = turn;

  return (
    <article className="min-w-0">
      <div className="min-w-0 border-l pl-4">
        <div className="mb-2 flex items-center gap-2 text-[13px] font-medium text-muted-foreground">
          <span>{roleLabels.assistant}</span>
          {assistantTurn.streaming && (
            <span className="thinking-dots" aria-label="正在生成" />
          )}
          {usage && (
            <span className="rounded-full border bg-background px-2 py-0.5 font-mono text-[11px]">
              {formatTokenCount(usage.totalTokens)} tokens
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
        <Button type="button" onClick={onConfigureApiKey} disabled={savingKey || !apiKeyDraft.trim()}>
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
