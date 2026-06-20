import { Bot, KeyRound } from "lucide-react";
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

const roleClasses: Record<ChatMessage["role"], string> = {
  user: "ml-auto border-primary bg-primary text-primary-foreground",
  assistant: "mr-auto bg-card text-card-foreground",
  thinking: "mr-auto border-emerald-200 bg-emerald-50 text-emerald-900",
  system: "mx-auto border-amber-200 bg-amber-50 text-amber-900",
};

const roleLabels: Record<ChatMessage["role"], string> = {
  user: "你",
  assistant: "telos",
  thinking: "思考",
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
  return (
    <ScrollArea className="min-h-0 w-full min-w-0">
      <section className="flex min-h-full w-full min-w-0 flex-col gap-3 px-5 py-7 md:px-[7vw]">
        {messages.length === 0 ? (
          <div className="m-auto w-full max-w-2xl">
            <div className="mb-5 flex size-11 items-center justify-center rounded-lg border bg-card text-card-foreground shadow-sm">
              <Bot className="size-5" aria-hidden="true" />
            </div>
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
        ) : (
          messages.map((message) => (
            <article
              className={cn(
                "w-full max-w-3xl whitespace-pre-wrap rounded-lg border px-4 py-3 shadow-sm",
                message.role === "user" ? "md:max-w-[720px]" : "md:max-w-[820px]",
                roleClasses[message.role],
              )}
              key={message.id}
            >
              <div className="mb-2 font-mono text-[11px] font-bold uppercase opacity-70">
                {roleLabels[message.role]}
              </div>
              <p className="m-0 leading-7">{message.content}</p>
            </article>
          ))
        )}
      </section>
    </ScrollArea>
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
    <div className="max-w-xl">
      <p className="mb-2 font-mono text-xs font-bold uppercase text-muted-foreground">
        首次设置
      </p>
      <h2 className="max-w-xl text-4xl font-semibold leading-tight tracking-normal text-foreground">
        先配置 DeepSeek API Key。
      </h2>
      <p className="mt-3 max-w-xl text-base leading-7 text-muted-foreground">
        telos 桌面端会把密钥写入 CLI 共用的用户配置文件。配置完成后，项目配置、记忆和工作目录会按 CLI 规则继续生效。
      </p>
      <div className="mt-6 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
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
    <>
      <p className="mb-2 font-mono text-xs font-bold uppercase text-muted-foreground">
        Agent 控制台
      </p>
      <h2 className="max-w-xl text-4xl font-semibold leading-tight tracking-normal text-foreground">
        直接给 telos 一个明确任务。
      </h2>
      <p className="mt-3 max-w-xl text-base leading-7 text-muted-foreground">
        可以让它检查当前工作区、解释变更、修改代码或执行验证。右侧只保留运行状态和工具活动，设置入口在顶部。
      </p>
      <div className="mt-6 flex flex-wrap gap-2">
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
    </>
  );
}
