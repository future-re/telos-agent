import { lazy, Suspense, useMemo, useState } from "react";
import { PanelLeftOpen, PanelRightOpen } from "lucide-react";
import { AgentStatusRail } from "@/components/AgentStatusRail";
import { Composer } from "@/components/Composer";
import { Conversation } from "@/components/Conversation";
import { TopBar } from "@/components/TopBar";

const SideWorkspace = lazy(() =>
  import("@/components/SideWorkspace").then((m) => ({ default: m.SideWorkspace })),
);
const MemoryOverviewDialog = lazy(() =>
  import("@/components/MemoryOverviewDialog").then((m) => ({ default: m.MemoryOverviewDialog })),
);
const ApprovalDialog = lazy(() =>
  import("@/components/ApprovalDialog").then((m) => ({ default: m.ApprovalDialog })),
);
import { TooltipProvider } from "@/components/ui/tooltip";
import { groupConversationMessages } from "@/conversationView";
import { cn } from "@/lib/utils";
import { buildRunDisplay } from "@/runDisplay";
import { sumTokenUsage } from "@/tokenUsage";
import { useAgentCommands } from "@/useAgentCommands";
import { useAppearanceSettings } from "@/useAppearanceSettings";
import { useApprovals } from "@/useApprovals";
import { ChatAction, useConversationSessions } from "@/useConversationSessions";
import { useDeepSeekSync } from "@/useDeepSeekSync";
import { useDesktopSettings } from "@/useDesktopSettings";
import { useMemoryOverview } from "@/useMemoryOverview";
import { useTelosEventQueue } from "@/useTelosEventQueue";
import { useTokenUsageHistory } from "@/useTokenUsageHistory";
import { useWorkspacePanel } from "@/useWorkspacePanel";

export function App() {
  const {
    activeSessionId,
    applyTelosEvents,
    appendDeepSeekSyncMessage,
    createNewConversation,
    deleteConversation,
    dispatchChatAction,
    resetAllSessionStates,
    selectSession,
    sessions,
    state,
    startPrompt,
  } = useConversationSessions();
  const [prompt, setPrompt] = useState("");
  const { appearance, setAppearance } = useAppearanceSettings();
  const {
    agentRailOpen,
    agentRailWidth,
    inspectorOpen,
    openDeepSeekPanel,
    openSettings,
    setSettingsOpen,
    setSettingsSection,
    setSideWorkspaceTab,
    settingsOpen,
    settingsSection,
    sideWorkspaceTab,
    sideWorkspaceWidth,
    startAgentRailResize,
    startSideWorkspaceResize,
    toggleAgentRail,
    toggleInspector,
  } = useWorkspacePanel();
  const resetSessionsAndApprovals = () => {
    resetAllSessionStates();
    clearAllApprovals();
  };
  const {
    apiKeyDraft,
    chooseDirectory,
    effectiveSettings,
    loadingSettings,
    normalizeOverrides,
    overrides,
    saveApiKey,
    savingKey,
    setApiKeyDraft,
    settings,
    updateOverrides,
  } = useDesktopSettings({
    onError: (message) => dispatch({ type: "error", message }),
    onResetSessions: resetSessionsAndApprovals,
  });
  const { recordUsageEvent, todayUsage, usageHistory } =
    useTokenUsageHistory();
  const {
    approvalDraft,
    approvalError,
    pendingApproval,
    addApprovalFromEvent,
    clearAllApprovals,
    clearApproval,
    resolveApproval,
    setApprovalDraft,
    setApprovalError,
  } = useApprovals({
    activeSessionId,
    onResolveError: (sessionId, message) => {
      dispatch({ type: "error", message }, sessionId);
    },
  });
  function dispatch(action: ChatAction, sessionId = activeSessionId) {
    if (action.type === "event" && action.event.kind === "provider_usage") {
      recordUsageEvent(action.event);
    }

    dispatchChatAction(action, sessionId);
  }

  useTelosEventQueue({
    activeSessionId,
    onApprovalRequired: (sessionId, event) => {
      addApprovalFromEvent(sessionId, event);
      selectSession(sessionId);
    },
    onEvents: applyTelosEvents,
    onProviderUsage: recordUsageEvent,
  });

  const deepseekNeedsKey =
    effectiveSettings.provider === "deepseek" &&
    !effectiveSettings.apiKeyConfigured;
  const { consumeSyncedContext, syncDeepSeek } = useDeepSeekSync({
    appendSyncMessage: appendDeepSeekSyncMessage,
    disabled: deepseekNeedsKey,
  });
  const {
    removeConversation,
    startNewConversation,
    stopCurrentTask,
    submit,
  } = useAgentCommands({
    activeSessionId,
    clearApproval,
    consumeSyncedContext,
    createNewConversation,
    deepseekNeedsKey,
    deleteConversation,
    dispatch,
    normalizeOverrides,
    prompt,
    running: state.running,
    setPrompt,
    startPrompt,
  });
  const {
    memoryLoading,
    memoryOpen,
    memoryOverview,
    openMemoryOverview,
    setMemoryOpen,
  } = useMemoryOverview({
    cwd: effectiveSettings.cwd,
    onError: (message) => dispatch({ type: "error", message }),
  });
  const canSend = useMemo(
    () => prompt.trim().length > 0 && !deepseekNeedsKey && !loadingSettings,
    [deepseekNeedsKey, loadingSettings, prompt],
  );
  const sessionUsage = useMemo(
    () => sumTokenUsage(Object.values(state.usageByTurnId)),
    [state.usageByTurnId],
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

  return (
    <TooltipProvider delayDuration={250}>
      <main
        style={
          {
            "--agent-rail-width": `${agentRailWidth}px`,
            "--side-workspace-width": `${sideWorkspaceWidth}px`,
          } as React.CSSProperties
        }
        className={cn(
          "relative grid h-screen w-full grid-rows-[auto_minmax(0,1fr)] overflow-hidden bg-background text-foreground",
        )}
      >
        <TopBar
          agentRailOpen={agentRailOpen}
          apiKeyDraft={apiKeyDraft}
          metadata={display.runMetadata}
          onApiKeyDraftChange={setApiKeyDraft}
          onOverridesChange={updateOverrides}
          onAppearanceChange={setAppearance}
          onNewConversation={startNewConversation}
          onSaveApiKey={saveApiKey}
          onOpenDeepSeek={openDeepSeekPanel}
          onToggleAgentRail={toggleAgentRail}
          onTogglePanel={toggleInspector}
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
          sideWorkspaceTab={sideWorkspaceTab}
          appearance={appearance}
          turnUsage={state.currentTurnUsage}
          turnModel={state.currentTurnUsage?.model}
        />

        <section
          className={cn(
            "relative grid min-h-0 w-full min-w-0 overflow-hidden",
            inspectorOpen
              ? "lg:grid-cols-[minmax(0,1fr)_var(--side-workspace-width)]"
              : "grid-cols-1",
          )}
        >
          <div
            className={cn(
              "relative grid h-full min-h-0 min-w-0 overflow-hidden grid-cols-1",
              agentRailOpen &&
                "min-[1180px]:grid-cols-[var(--agent-rail-width)_minmax(0,1fr)]",
            )}
          >
            {agentRailOpen && (
              <AgentStatusRail
                activeSessionId={activeSessionId}
                sessions={sessions}
                onDeleteSession={removeConversation}
                onNewSession={startNewConversation}
                onSelectSession={selectSession}
                running={state.running}
                status={state.status}
                tools={state.tools}
              />
            )}
            {agentRailOpen && (
              <div
                className="absolute inset-y-0 z-20 hidden w-3 cursor-col-resize min-[1180px]:block"
                style={{ left: "var(--agent-rail-width)" }}
                role="separator"
                aria-label="调整左侧面板宽度"
                aria-orientation="vertical"
                onMouseDown={startAgentRailResize}
              >
                <span className="absolute inset-y-0 left-0 w-px bg-border/0 transition-colors hover:bg-ring/50" />
              </div>
            )}
            <div className="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto]">
              <Conversation
                messages={state.messages}
                needsApiKey={deepseekNeedsKey}
                usageByTurnId={state.usageByTurnId}
                onConfigureApiKey={saveApiKey}
                onApiKeyChange={setApiKeyDraft}
                apiKeyDraft={apiKeyDraft}
                savingKey={savingKey}
              />
              <Composer
                value={prompt}
                sendDisabled={!canSend}
                disabledReason={
                  deepseekNeedsKey ? "请先配置 DeepSeek API Key" : undefined
                }
                running={state.running}
                onChange={setPrompt}
                onStop={stopCurrentTask}
                onSubmit={submit}
              />
            </div>
          </div>

          {inspectorOpen && (
            <Suspense fallback={null}>
              <div
                className="absolute inset-y-0 z-20 hidden w-3 cursor-col-resize lg:block"
                style={{ right: "var(--side-workspace-width)" }}
                role="separator"
                aria-label="调整侧边栏宽度"
                aria-orientation="vertical"
                onMouseDown={startSideWorkspaceResize}
              >
                <span className="absolute inset-y-0 right-0 w-px bg-border/0 transition-colors hover:bg-ring/50" />
              </div>
              <SideWorkspace
                activeTab={sideWorkspaceTab}
                display={display}
                onChooseDirectory={chooseDirectory}
                onConfigure={openSettings}
                onOpenMemory={openMemoryOverview}
                onSyncDeepSeek={syncDeepSeek}
                onTabChange={setSideWorkspaceTab}
                running={state.running}
                status={state.status}
                tools={state.tools}
              />
            </Suspense>
          )}
          {!agentRailOpen && (
            <button
              type="button"
              className="absolute left-0 top-1/2 z-30 hidden h-14 w-8 -translate-y-1/2 items-center justify-center rounded-r-lg border-y border-r bg-card/95 text-muted-foreground shadow-[0_10px_24px_rgba(15,23,42,0.12)] transition-colors hover:text-foreground min-[1180px]:flex"
              aria-label="展开左侧面板"
              onClick={toggleAgentRail}
            >
              <PanelLeftOpen className="size-4" aria-hidden="true" />
            </button>
          )}
          {!inspectorOpen && (
            <button
              type="button"
              className="absolute right-0 top-1/2 z-30 hidden h-14 w-8 -translate-y-1/2 items-center justify-center rounded-l-lg border-y border-l bg-card/95 text-muted-foreground shadow-[0_10px_24px_rgba(15,23,42,0.12)] transition-colors hover:text-foreground lg:flex"
              aria-label="展开右侧面板"
              onClick={toggleInspector}
            >
              <PanelRightOpen className="size-4" aria-hidden="true" />
            </button>
          )}
        </section>
      </main>
      {memoryOpen && (
        <Suspense fallback={null}>
          <MemoryOverviewDialog
            loading={memoryLoading}
            memory={memoryOverview}
            open={memoryOpen}
            onOpenChange={setMemoryOpen}
          />
        </Suspense>
      )}
      {pendingApproval && (
        <Suspense fallback={null}>
          <ApprovalDialog
            approval={pendingApproval}
            draft={approvalDraft}
            error={approvalError}
            onDraftChange={(value: string) => {
              setApprovalDraft(value);
              setApprovalError("");
            }}
            onApprove={() => resolveApproval("allow")}
            onDeny={() => resolveApproval("deny")}
            onModify={() => resolveApproval("modify")}
          />
        </Suspense>
      )}
    </TooltipProvider>
  );
}
