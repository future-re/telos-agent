import { FormEvent, useEffect, useMemo, useReducer, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ChatState,
  TelosEvent,
  initialChatState,
  reduceTelosEvent,
  startUserTurn,
} from "@/chatState";
import { Composer } from "@/components/Composer";
import { Conversation } from "@/components/Conversation";
import { RunInspector } from "@/components/RunInspector";
import { TopBar } from "@/components/TopBar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { DesktopSettingsOverrides, ResolvedDesktopSettings } from "@/desktopTypes";
import { buildRunDisplay } from "@/runDisplay";

interface PromptResult {
  finalText: string;
}

type Action =
  | { type: "start"; prompt: string }
  | { type: "event"; event: TelosEvent }
  | { type: "error"; message: string }
  | { type: "reset" };

function reducer(state: ChatState, action: Action): ChatState {
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
  const [state, dispatch] = useReducer(reducer, initialChatState);
  const [prompt, setPrompt] = useState("");
  const [settings, setSettings] = useState<ResolvedDesktopSettings>(fallbackSettings);
  const [overrides, setOverrides] = useState<DesktopSettingsOverrides>({});
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);

  useEffect(() => {
    const unlisten = listen<TelosEvent>("telos://event", (event) => {
      dispatch({ type: "event", event: event.payload });
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
    () => prompt.trim().length > 0 && !state.running && !deepseekNeedsKey && !loadingSettings,
    [deepseekNeedsKey, loadingSettings, prompt, state.running],
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
    setPrompt("");
    dispatch({ type: "start", prompt: text });
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

  async function resetSession() {
    await invoke("reset_session").catch(() => undefined);
    dispatch({ type: "reset" });
  }

  return (
    <TooltipProvider delayDuration={250}>
      <main className="grid min-h-screen w-full overflow-x-hidden bg-muted/40 text-foreground lg:grid-cols-[minmax(0,1fr)_minmax(352px,388px)]">
        <section className="grid min-h-screen w-full min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] bg-background">
          <TopBar
            apiKeyDraft={apiKeyDraft}
            metadata={display.runMetadata}
            onApiKeyDraftChange={setApiKeyDraft}
            onOverridesChange={updateOverrides}
            onReset={resetSession}
            onSaveApiKey={saveApiKey}
            onTogglePanel={() => setInspectorOpen((open) => !open)}
            overrides={overrides}
            panelOpen={inspectorOpen}
            savingKey={savingKey}
            settings={settings}
          />
          <Conversation
            messages={state.messages}
            needsApiKey={deepseekNeedsKey}
            onConfigureApiKey={saveApiKey}
            onApiKeyChange={setApiKeyDraft}
            apiKeyDraft={apiKeyDraft}
            savingKey={savingKey}
            onPickPrompt={setPrompt}
          />
          <Composer
            value={prompt}
            disabled={!canSend}
            disabledReason={deepseekNeedsKey ? "请先配置 DeepSeek API Key" : undefined}
            onChange={setPrompt}
            onSubmit={submit}
          />
        </section>

        {inspectorOpen && (
          <RunInspector
            display={display}
            running={state.running}
            status={state.status}
            tools={state.tools}
          />
        )}
      </main>
    </TooltipProvider>
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
