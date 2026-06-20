import { useEffect, useRef } from "react";
import { Bot, KeyRound, UserRound } from "lucide-react";
import { ChatMessage } from "@/chatState";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

interface ConversationProps {
  messages: ChatMessage[];
  needsApiKey: boolean;
  apiKeyDraft: string;
  savingKey: boolean;
  onApiKeyChange: (value: string) => void;
  onConfigureApiKey: () => void;
  onPickPrompt: (prompt: string) => void;
}

const roleLabels: Record<ChatMessage["role"], string> = {
  user: "你",
  assistant: "telos",
  thinking: "思考中",
  system: "系统",
};

export function Conversation({
  apiKeyDraft,
  messages,
  needsApiKey,
  onApiKeyChange,
  onConfigureApiKey,
  onPickPrompt,
  savingKey,
}: ConversationProps) {
  const endRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [messages]);

  return (
    <section className="h-full min-h-0 overflow-hidden bg-muted/20">
      <ScrollArea className="h-full w-full">
        <div className="mx-auto flex min-h-full w-full max-w-4xl flex-col px-4 py-6 md:px-6">
          {messages.length === 0 ? (
            <div className="flex min-h-[420px] flex-1 items-center">
              <div className="w-full">
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
            <div className="flex min-h-0 flex-1 flex-col gap-4">
              {messages.map((message) => (
                <MessageBubble key={message.id} message={message} />
              ))}
              <div ref={endRef} />
            </div>
          )}
        </div>
      </ScrollArea>
    </section>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === "user";
  const isSystem = message.role === "system";
  const isThinking = message.role === "thinking";

  return (
    <article
      className={cn(
        "flex w-full gap-3",
        isUser && "justify-end",
        isSystem && "justify-center",
      )}
    >
      {!isUser && !isSystem && (
        <Avatar className={isThinking ? "border-emerald-200 bg-emerald-50 text-emerald-700" : ""}>
          <Bot className="size-4" aria-hidden="true" />
        </Avatar>
      )}
      <div
        className={cn(
          "min-w-0 max-w-[min(760px,calc(100%-3rem))] rounded-lg border px-4 py-3 shadow-sm",
          isUser && "bg-primary text-primary-foreground",
          message.role === "assistant" && "bg-card text-card-foreground",
          isThinking && "border-emerald-200 bg-emerald-50 text-emerald-950",
          isSystem && "max-w-2xl border-amber-200 bg-amber-50 text-amber-950",
        )}
      >
        <div className="mb-1.5 text-xs font-medium opacity-70">{roleLabels[message.role]}</div>
        <div className="whitespace-pre-wrap break-words text-sm leading-7">{message.content}</div>
      </div>
      {isUser && (
        <Avatar className="border-primary bg-primary text-primary-foreground">
          <UserRound className="size-4" aria-hidden="true" />
        </Avatar>
      )}
    </article>
  );
}

function Avatar({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span
      className={cn(
        "mt-1 flex size-8 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground",
        className,
      )}
    >
      {children}
    </span>
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
    <div className="mx-auto w-full max-w-xl rounded-lg border bg-background p-5 shadow-sm">
      <div className="mb-4 flex size-10 items-center justify-center rounded-md border bg-card">
        <Bot className="size-5" aria-hidden="true" />
      </div>
      <p className="mb-2 text-xs font-medium text-muted-foreground">首次设置</p>
      <h2 className="text-2xl font-semibold leading-tight tracking-normal text-foreground">
        配置 DeepSeek API Key
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted-foreground">
        密钥会写入 CLI 共用的用户配置文件。配置完成后，项目配置、记忆和工作目录会按 CLI 规则生效。
      </p>
      <div className="mt-5 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
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
    <div className="mx-auto w-full max-w-2xl rounded-lg border bg-background p-5 shadow-sm">
      <div className="mb-4 flex size-10 items-center justify-center rounded-md border bg-card">
        <Bot className="size-5" aria-hidden="true" />
      </div>
      <p className="mb-2 text-xs font-medium text-muted-foreground">对话面板</p>
      <h2 className="text-2xl font-semibold leading-tight tracking-normal text-foreground">
        给 telos 一个明确任务
      </h2>
      <p className="mt-3 text-sm leading-6 text-muted-foreground">
        可以检查当前工作区、解释变更、修改代码或执行验证。消息会固定在这个对话面板中滚动。
      </p>
      <div className="mt-5 flex flex-wrap gap-2">
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
