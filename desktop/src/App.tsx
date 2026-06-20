import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  AppearanceSettings,
  applyAppearance,
  loadAppearance,
  saveAppearance,
} from "@/appearance";
import {
  ChatState,
  TelosEvent,
  initialChatState,
  reduceTelosEvent,
  startUserTurn,
} from "@/chatState";
import { defaultAgent } from "@/agentModel";
import { AgentStatusRail } from "@/components/AgentStatusRail";
import { Composer } from "@/components/Composer";
import { Conversation } from "@/components/Conversation";
import { MemoryOverviewDialog } from "@/components/MemoryOverviewDialog";
import { RunInspector } from "@/components/RunInspector";
import { TopBar } from "@/components/TopBar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import {
  ConversationSession,
  createConversationSession,
  deleteConversationSession,
  renameSessionFromPrompt,
  updateSessionState,
} from "@/conversationSession";
import {
  DesktopSettingsOverrides,
  MemoryOverview,
  ResolvedDesktopSettings,
  SettingsSection,
} from "@/desktopTypes";
import { groupConversationMessages } from "@/conversationView";
import { cn } from "@/lib/utils";
import { buildRunDisplay } from "@/runDisplay";
import { sumTokenUsage } from "@/tokenUsage";
import {
  TokenUsageHistory,
  addUsageToHistory,
  dateKey,
  loadTokenUsageHistory,
  saveTokenUsageHistory,
} from "@/tokenUsageHistory";

interface PromptResult {
  finalText: string;
}

interface PendingApproval {
  approvalId: string;
  toolName: string;
  arguments: unknown;
  cwd?: string;
  reason?: string;
}

type Action =
  | { type: "start"; prompt: string }
  | { type: "event"; event: TelosEvent }
  | { type: "error"; message: string }
  | { type: "reset" };

function reduceChatAction(state: ChatState, action: Action): ChatState {
  switch (action.type) {
    case "start":
      return startUserTurn(state, action.prompt);
    case "event":
      return reduceTelosEvent(state, action.event);
    case "error":
      return {
        ...state,
        running: false,
        status: action.message,
        messages: [
          ...state.messages,
          {
            id: `error-${Date.now()}`,
            role: "system",
            content: action.message,
          },
        ],
      };
    case "reset":
      return initialChatState;
  }
}

const fallbackSettings: ResolvedDesktopSettings = {
  provider: "deepseek",
  model: "auto",
  cwd: "",
  projectRootOrCwd: "",
  memoryRoot: "",
  memoryCount: 0,
  apiKeyConfigured: false,
  autoApprove: false,
  maxIterations: 30,
};

export function App() {
  const initialSession = useMemo(() => createConversationSession("session-1"), []);
  const [sessions, setSessions] = useState<ConversationSession[]>([initialSession]);
  const [activeSessionId, setActiveSessionId] = useState(initialSession.id);
  const [prompt, setPrompt] = useState("");
  const [settings, setSettings] = useState<ResolvedDesktopSettings>(fallbackSettings);
  const [overrides, setOverrides] = useState<DesktopSettingsOverrides>({});
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("appearance");
  const [appearance, setAppearance] = useState<AppearanceSettings>(() => loadAppearance());
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [memoryLoading, setMemoryLoading] = useState(false);
  const [memoryOverview, setMemoryOverview] = useState<MemoryOverview | undefined>();
  const [pendingApproval, setPendingApproval] = useState<PendingApproval | undefined>();
  const [approvalDraft, setApprovalDraft] = useState("");
  const [approvalError, setApprovalError] = useState("");
  const [usageHistory, setUsageHistory] = useState<TokenUsageHistory>(() =>
    typeof window === "undefined" ? {} : loadTokenUsageHistory(),
  );
  const runningSessionIdRef = useRef(activeSessionId);

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [activeSessionId, sessions],
  );
  const state = activeSession?.state ?? initialChatState;

  function dispatch(action: Action, sessionId = activeSessionId) {
    if (action.type === "event" && action.event.kind === "provider_usage") {
      const usage = usageFromEvent(action.event);
      if (usage) {
        setUsageHistory((current) => {
          const next = addUsageToHistory(current, usage);
          saveTokenUsageHistory(next);
          return next;
        });
      }
    }

    setSessions((current) =>
      updateSessionState(current, sessionId, (chatState) => reduceChatAction(chatState, action)),
    );
  }

  useEffect(() => {
    const unlisten = listen<TelosEvent>("telos://event", (event) => {
      const payload = event.payload;
      if (payload.kind === "approval_required" && payload.approvalId) {
        const nextApproval = {
          approvalId: payload.approvalId,
          toolName: payload.toolName ?? "Tool",
          arguments: payload.arguments ?? {},
          cwd: payload.cwd,
          reason: payload.reason,
        };
        setPendingApproval(nextApproval);
        setApprovalDraft(formatJson(nextApproval.arguments));
        setApprovalError("");
      }
      dispatch({ type: "event", event: payload }, runningSessionIdRef.current);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    refreshSettings().catch((error) => {
      dispatch({ type: "error", message: `读取配置失败：${String(error)}` });
    });
  }, []);

  useEffect(() => {
    applyAppearance(appearance);
    saveAppearance(appearance);
  }, [appearance]);

  const effectiveSettings = useMemo(
    () => ({
      ...settings,
      ...definedOverrides(overrides),
    }),
    [overrides, settings],
  );

  const deepseekNeedsKey =
    effectiveSettings.provider === "deepseek" && !effectiveSettings.apiKeyConfigured;
  const canSend = useMemo(
    () => prompt.trim().length > 0 && !deepseekNeedsKey && !loadingSettings,
    [deepseekNeedsKey, loadingSettings, prompt],
  );
  const turnCount = useMemo(
    () => groupConversationMessages(state.messages).length,
    [state.messages],
  );
  const sessionUsage = useMemo(
    () => sumTokenUsage(Object.values(state.usageByTurnId)),
    [state.usageByTurnId],
  );
  const todayUsage = useMemo(
    () => usageHistory[dateKey()],
    [usageHistory],
  );
  const display = buildRunDisplay({
    provider: effectiveSettings.provider,
    model: effectiveSettings.model,
    cwd: effectiveSettings.cwd,
    projectRoot: effectiveSettings.projectRoot,
    memoryCount: effectiveSettings.memoryCount,
    apiKeyConfigured: effectiveSettings.apiKeyConfigured,
    autoApprove: effectiveSettings.autoApprove,
    status: state.status,
    running: state.running,
  });

  async function refreshSettings(nextOverrides = overrides) {
    if (!isTauriRuntime()) {
      setSettings({
        ...fallbackSettings,
        cwd: nextOverrides.cwd ?? "浏览器预览模式",
        projectRootOrCwd: nextOverrides.cwd ?? "浏览器预览模式",
      });
      setLoadingSettings(false);
      return;
    }
    const resolved = await invoke<ResolvedDesktopSettings>("resolved_settings", {
      request: nextOverrides.cwd ? { cwd: nextOverrides.cwd } : undefined,
    });
    setSettings(resolved);
    setLoadingSettings(false);
  }

  function updateOverrides(next: DesktopSettingsOverrides) {
    setOverrides(next);
    if (next.cwd !== overrides.cwd) {
      refreshSettings(next).catch((error) => {
        dispatch({ type: "error", message: `刷新配置失败：${String(error)}` });
      });
    }
  }

  async function saveApiKey() {
    const apiKey = apiKeyDraft.trim();
    if (!apiKey) {
      return;
    }
    setSavingKey(true);
    try {
      if (!isTauriRuntime()) {
        setSettings((current) => ({ ...current, apiKeyConfigured: true }));
        setApiKeyDraft("");
        return;
      }
      const resolved = await invoke<ResolvedDesktopSettings>("save_deepseek_key", {
        request: { apiKey },
      });
      setSettings(resolved);
      setOverrides((current) => ({ ...current, provider: "deepseek", apiKey: undefined }));
      setApiKeyDraft("");
      await invoke("reset_session").catch(() => undefined);
      dispatch({ type: "reset" });
    } catch (error) {
      dispatch({ type: "error", message: `保存 API Key 失败：${String(error)}` });
    } finally {
      setSavingKey(false);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = prompt.trim();
    if (!text || state.running || deepseekNeedsKey) {
      return;
    }
    runningSessionIdRef.current = activeSessionId;
    setPrompt("");
    setSessions((current) =>
      updateSessionState(current, activeSessionId, (chatState) =>
        reduceChatAction(chatState, { type: "start", prompt: text }),
      ).map((session) =>
        session.id === activeSessionId ? renameSessionFromPrompt(session, text) : session,
      ),
    );
    try {
      await invoke<PromptResult>("send_prompt", {
        request: {
          prompt: text,
          settings: normalizeOverrides(overrides, settings),
        },
      });
    } catch (error) {
      dispatch({ type: "error", message: String(error) });
    }
  }

  async function stopCurrentTask() {
    if (isTauriRuntime()) {
      await invoke("cancel_current_task").catch((error) => {
        dispatch({ type: "error", message: `停止任务失败：${String(error)}` });
      });
    }
    dispatch(
      {
        type: "event",
        event: {
          kind: "cancelled",
          message: "已停止当前任务",
        },
      },
      runningSessionIdRef.current,
    );
    setPendingApproval(undefined);
    setApprovalError("");
  }

  async function resetSession() {
    await invoke("reset_session").catch(() => undefined);
    dispatch({ type: "reset" });
    setPendingApproval(undefined);
    setApprovalError("");
  }

  async function resolveApproval(decision: "allow" | "deny" | "modify") {
    if (!pendingApproval) {
      return;
    }

    let parsedArguments: unknown | undefined;
    if (decision === "modify") {
      try {
        parsedArguments = JSON.parse(approvalDraft);
      } catch (error) {
        setApprovalError(`JSON 无效：${String(error)}`);
        return;
      }
    }

    const approvalId = pendingApproval.approvalId;
    setPendingApproval(undefined);
    setApprovalError("");

    try {
      if (isTauriRuntime()) {
        await invoke("resolve_approval", {
          request: {
            approvalId,
            decision,
            arguments: parsedArguments,
          },
        });
      }
    } catch (error) {
      dispatch({ type: "error", message: `处理审批失败：${String(error)}` }, runningSessionIdRef.current);
    }
  }

  function createNewConversation() {
    const session = createConversationSession(`session-${Date.now()}`);
    setSessions((current) => [session, ...current]);
    setActiveSessionId(session.id);
    setPrompt("");
  }

  function deleteConversation(sessionId: string) {
    const session = sessions.find((item) => item.id === sessionId);
    if (session?.state.running) {
      return;
    }

    setSessions((current) => {
      const result = deleteConversationSession(current, sessionId, activeSessionId);
      setActiveSessionId(result.activeSessionId);
      return result.sessions;
    });
  }

  function openSettings(section: SettingsSection) {
    setSettingsSection(section);
    setSettingsOpen(true);
  }

  async function chooseDirectory() {
    if (!isTauriRuntime()) {
      const selected = window.prompt("输入工作目录", effectiveSettings.cwd);
      if (selected?.trim()) {
        const next = { ...overrides, cwd: selected.trim() };
        setOverrides(next);
        await refreshSettings(next);
      }
      return;
    }

    const selected = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: effectiveSettings.cwd || undefined,
      title: "选择工作目录",
    });
    if (typeof selected !== "string" || !selected.trim()) {
      return;
    }
    const next = { ...overrides, cwd: selected };
    setOverrides(next);
    await refreshSettings(next);
    await invoke("reset_session").catch(() => undefined);
    dispatch({ type: "reset" });
  }

  async function openMemoryOverview() {
    setMemoryOpen(true);
    setMemoryLoading(true);
    try {
      if (!isTauriRuntime()) {
        setMemoryOverview({
          root: "浏览器预览模式",
          total: 0,
          categories: [
            { label: "事实", count: 0 },
            { label: "命令", count: 0 },
            { label: "流程", count: 0 },
            { label: "模式", count: 0 },
            { label: "脚本", count: 0 },
          ],
          statuses: [
            { label: "可用", count: 0 },
            { label: "需确认", count: 0 },
            { label: "已废弃", count: 0 },
          ],
          recent: [],
        });
        return;
      }

      const overview = await invoke<MemoryOverview>("memory_summary", {
        request: effectiveSettings.cwd ? { cwd: effectiveSettings.cwd } : undefined,
      });
      setMemoryOverview(overview);
    } catch (error) {
      dispatch({ type: "error", message: `读取记忆失败：${String(error)}` });
      setMemoryOverview(undefined);
    } finally {
      setMemoryLoading(false);
    }
  }

  return (
    <TooltipProvider delayDuration={250}>
      <main
        className={cn(
          "grid h-screen w-full overflow-hidden bg-muted/40 text-foreground",
          inspectorOpen
            ? "lg:grid-cols-[minmax(0,1fr)_minmax(300px,336px)]"
            : "grid-cols-1",
        )}
      >
        <section className="grid h-screen w-full min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden bg-background">
          <TopBar
            apiKeyDraft={apiKeyDraft}
            metadata={display.runMetadata}
            onApiKeyDraftChange={setApiKeyDraft}
            onOverridesChange={updateOverrides}
            onAppearanceChange={setAppearance}
            onReset={resetSession}
            onSaveApiKey={saveApiKey}
            onTogglePanel={() => setInspectorOpen((open) => !open)}
            overrides={overrides}
            panelOpen={inspectorOpen}
            savingKey={savingKey}
            sessionUsage={sessionUsage}
            settings={settings}
            settingsOpen={settingsOpen}
            todayUsage={todayUsage}
            tokenHistory={usageHistory}
            onSettingsOpenChange={setSettingsOpen}
            settingsSection={settingsSection}
            onSettingsSectionChange={setSettingsSection}
            appearance={appearance}
            turnUsage={state.currentTurnUsage}
          />
          <div className="grid min-h-0 min-w-0 grid-cols-1 min-[1180px]:grid-cols-[340px_minmax(0,1fr)]">
            <AgentStatusRail
              agent={defaultAgent}
              activeSessionId={activeSessionId}
              sessions={sessions}
              onDeleteSession={deleteConversation}
              onNewSession={createNewConversation}
              onSelectSession={setActiveSessionId}
              running={state.running}
              status={state.status}
              tools={state.tools}
              turnCount={turnCount}
            />
            <div className="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto]">
              <Conversation
                messages={state.messages}
                needsApiKey={deepseekNeedsKey}
                usageByTurnId={state.usageByTurnId}
                onConfigureApiKey={saveApiKey}
                onApiKeyChange={setApiKeyDraft}
                apiKeyDraft={apiKeyDraft}
                savingKey={savingKey}
                onPickPrompt={setPrompt}
              />
              <Composer
                value={prompt}
                sendDisabled={!canSend}
                disabledReason={deepseekNeedsKey ? "请先配置 DeepSeek API Key" : undefined}
                running={state.running}
                onChange={setPrompt}
                onStop={stopCurrentTask}
                onSubmit={submit}
              />
            </div>
          </div>
        </section>

        {inspectorOpen && (
          <RunInspector
            display={display}
            onChooseDirectory={chooseDirectory}
            onConfigure={openSettings}
            onOpenMemory={openMemoryOverview}
            running={state.running}
            status={state.status}
            tools={state.tools}
          />
        )}
      </main>
      <MemoryOverviewDialog
        loading={memoryLoading}
        memory={memoryOverview}
        open={memoryOpen}
        onOpenChange={setMemoryOpen}
      />
      <ApprovalDialog
        approval={pendingApproval}
        draft={approvalDraft}
        error={approvalError}
        onDraftChange={(value) => {
          setApprovalDraft(value);
          setApprovalError("");
        }}
        onApprove={() => resolveApproval("allow")}
        onDeny={() => resolveApproval("deny")}
        onModify={() => resolveApproval("modify")}
      />
    </TooltipProvider>
  );
}

function ApprovalDialog({
  approval,
  draft,
  error,
  onApprove,
  onDeny,
  onDraftChange,
  onModify,
}: {
  approval?: PendingApproval;
  draft: string;
  error: string;
  onApprove: () => void;
  onDeny: () => void;
  onDraftChange: (value: string) => void;
  onModify: () => void;
}) {
  return (
    <Dialog
      open={Boolean(approval)}
      onOpenChange={(open) => {
        if (!open && approval) {
          onDeny();
        }
      }}
    >
      <DialogContent
        className="max-w-2xl"
        onEscapeKeyDown={(event) => event.preventDefault()}
        onPointerDownOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            命令需要确认
            {approval?.toolName ? <Badge variant="outline">{approval.toolName}</Badge> : null}
          </DialogTitle>
          <DialogDescription>
            自动批准关闭时，需审批的工具调用会暂停在这里等待你的决定。
          </DialogDescription>
        </DialogHeader>

        {approval && (
          <div className="grid gap-3">
            <div className="grid gap-1 rounded-md border bg-muted/30 p-3 text-sm">
              <span className="font-medium">原因</span>
              <span className="text-muted-foreground">{approval.reason || "需要人工确认"}</span>
            </div>
            {approval.cwd ? (
              <div className="grid gap-1 rounded-md border bg-muted/30 p-3 text-sm">
                <span className="font-medium">工作目录</span>
                <span className="break-all font-mono text-xs text-muted-foreground">
                  {approval.cwd}
                </span>
              </div>
            ) : null}
            <label className="grid gap-1.5">
              <span className="text-sm font-medium">调用参数</span>
              <Textarea
                className="min-h-44 font-mono text-xs leading-5"
                value={draft}
                onChange={(event) => onDraftChange(event.target.value)}
                spellCheck={false}
              />
            </label>
            {error ? <p className="text-sm text-red-600">{error}</p> : null}
          </div>
        )}

        <DialogFooter>
          <Button type="button" variant="destructive" onClick={onDeny}>
            拒绝
          </Button>
          <Button type="button" variant="outline" onClick={onModify}>
            按编辑参数批准
          </Button>
          <Button type="button" onClick={onApprove}>
            批准
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function normalizeOverrides(
  overrides: DesktopSettingsOverrides,
  settings: ResolvedDesktopSettings,
): DesktopSettingsOverrides {
  return {
    provider: overrides.provider ?? settings.provider,
    apiKey: overrides.apiKey?.trim() || undefined,
    cwd: overrides.cwd?.trim() || settings.cwd || undefined,
    model: overrides.model?.trim() || settings.model || "auto",
    maxIterations: overrides.maxIterations ?? settings.maxIterations,
    autoApprove: overrides.autoApprove ?? settings.autoApprove,
  };
}

function definedOverrides(overrides: DesktopSettingsOverrides) {
  return Object.fromEntries(
    Object.entries(overrides).filter(([, value]) => value !== undefined && value !== ""),
  ) as Partial<ResolvedDesktopSettings>;
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function usageFromEvent(event: TelosEvent) {
  if (event.inputTokens === undefined || event.outputTokens === undefined) {
    return undefined;
  }

  return {
    inputTokens: event.inputTokens,
    outputTokens: event.outputTokens,
    totalTokens: event.totalTokens ?? event.inputTokens + event.outputTokens,
    promptCacheHitTokens: event.promptCacheHitTokens,
    promptCacheMissTokens: event.promptCacheMissTokens,
    reasoningTokens: event.reasoningTokens,
  };
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value ?? {}, null, 2);
  } catch {
    return "{}";
  }
}
