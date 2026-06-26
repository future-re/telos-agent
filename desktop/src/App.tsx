import { useEffect, useMemo, useState } from "react";
import {
  AppearanceSettings,
  applyAppearance,
  loadAppearance,
  saveAppearance,
} from "@/appearance";
import { AgentStatusRail } from "@/components/AgentStatusRail";
import { ApprovalDialog } from "@/components/ApprovalDialog";
import { Composer } from "@/components/Composer";
import { Conversation } from "@/components/Conversation";
import { MemoryOverviewDialog } from "@/components/MemoryOverviewDialog";
import { SideWorkspace, SideWorkspaceTab } from "@/components/SideWorkspace";
import { TopBar } from "@/components/TopBar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SettingsSection } from "@/desktopTypes";
import { groupConversationMessages } from "@/conversationView";
import { cn } from "@/lib/utils";
import { buildRunDisplay } from "@/runDisplay";
import { sumTokenUsage } from "@/tokenUsage";
import { useAgentCommands } from "@/useAgentCommands";
import { useApprovals } from "@/useApprovals";
import { ChatAction, useConversationSessions } from "@/useConversationSessions";
import { useDeepSeekSync } from "@/useDeepSeekSync";
import { useDesktopSettings } from "@/useDesktopSettings";
import { useMemoryOverview } from "@/useMemoryOverview";
import { useSideWorkspaceResize } from "@/useSideWorkspaceResize";
import { useTelosEventQueue } from "@/useTelosEventQueue";
import { useTokenUsageHistory } from "@/useTokenUsageHistory";

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
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [sideWorkspaceTab, setSideWorkspaceTab] =
    useState<SideWorkspaceTab>("run");
  const { sideWorkspaceWidth, startSideWorkspaceResize } =
    useSideWorkspaceResize();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("appearance");
  const [appearance, setAppearance] = useState<AppearanceSettings>(() =>
    loadAppearance(),
  );
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

  useEffect(() => {
    applyAppearance(appearance);
    saveAppearance(appearance);
  }, [appearance]);

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

  function openSettings(section: SettingsSection) {
    setSettingsSection(section);
    setSettingsOpen(true);
    setInspectorOpen(true);
    setSideWorkspaceTab("run");
  }

  function openDeepSeekPanel() {
    setInspectorOpen(true);
    setSideWorkspaceTab("deepseek");
  }

  return (
    <TooltipProvider delayDuration={250}>
      <main
        style={
          inspectorOpen
            ? ({
                "--side-workspace-width": `${sideWorkspaceWidth}px`,
              } as React.CSSProperties)
            : undefined
        }
        className={cn(
          "grid h-screen w-full overflow-hidden bg-muted/40 text-foreground",
          inspectorOpen
            ? "lg:grid-cols-[minmax(0,1fr)_8px_var(--side-workspace-width)]"
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
            onNewConversation={startNewConversation}
            onSaveApiKey={saveApiKey}
            onOpenDeepSeek={openDeepSeekPanel}
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
            sideWorkspaceTab={sideWorkspaceTab}
            appearance={appearance}
            turnUsage={state.currentTurnUsage}
            turnModel={state.currentTurnUsage?.model}
          />
          <div className="grid min-h-0 min-w-0 grid-cols-1 min-[1180px]:grid-cols-[340px_minmax(0,1fr)]">
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
        </section>

        {inspectorOpen && (
          <>
            <div
              className="hidden bg-border/70 transition-colors hover:bg-ring/50 lg:block"
              role="separator"
              aria-label="调整侧边栏宽度"
              aria-orientation="vertical"
              onMouseDown={startSideWorkspaceResize}
            />
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
          </>
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
