import React, { FormEvent, useEffect, useMemo, useReducer, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ChatState,
  TelosEvent,
  initialChatState,
  reduceTelosEvent,
  startUserTurn,
} from "./chatState";
import "./styles.css";

type ProviderKind = "mock" | "deepseek";

interface ChatSettings {
  provider: ProviderKind;
  apiKey?: string;
  model?: string;
  cwd?: string;
  maxIterations?: number;
  autoApprove: boolean;
}

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

function App() {
  const [state, dispatch] = useReducer(reducer, initialChatState);
  const [prompt, setPrompt] = useState("");
  const [settings, setSettings] = useState<ChatSettings>({
    provider: "mock",
    model: "auto",
    maxIterations: 30,
    autoApprove: false,
  });
  const [settingsOpen, setSettingsOpen] = useState(true);

  useEffect(() => {
    const unlisten = listen<TelosEvent>("telos://event", (event) => {
      dispatch({ type: "event", event: event.payload });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const canSend = useMemo(() => prompt.trim().length > 0 && !state.running, [prompt, state.running]);
  const providerLabel = settings.provider === "deepseek" ? "DeepSeek" : "Mock";
  const modelLabel = settings.model?.trim() || "auto";
  const cwdLabel = settings.cwd?.trim() || "App launch directory";
  const approvalLabel = settings.autoApprove ? "Auto-approved" : "Manual approval";
  const runMetadata = `${providerLabel} · ${modelLabel} · ${state.status}`;

  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = prompt.trim();
    if (!text || state.running) {
      return;
    }
    setPrompt("");
    dispatch({ type: "start", prompt: text });
    try {
      await invoke<PromptResult>("send_prompt", {
        request: {
          prompt: text,
          settings: normalizeSettings(settings),
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
    <main className="app-shell">
      <section className="workspace">
        <header className="topbar">
          <div className="topbar-title">
            <h1>telos</h1>
            <span>{runMetadata}</span>
          </div>
          <div className="topbar-actions">
            <button
              type="button"
              className="secondary-action"
              onClick={() => setSettingsOpen((open) => !open)}
            >
              {settingsOpen ? "Hide Panel" : "Show Panel"}
            </button>
            <button type="button" className="secondary-action" onClick={resetSession}>
              New Chat
            </button>
          </div>
        </header>

        <section className="conversation" aria-live="polite">
          {state.messages.length === 0 ? (
            <div className="empty-state">
              <p className="eyebrow">Agent workspace</p>
              <h2>Start a focused session</h2>
              <p>
                Ask telos to inspect files, answer project questions, or run approved
                tools. Keep provider, workspace, approvals, and live tool activity in the
                run panel.
              </p>
              <div className="empty-prompts" aria-label="Example prompts">
                <span>Review this repo</span>
                <span>Explain the current change</span>
                <span>Run the relevant tests</span>
              </div>
            </div>
          ) : (
            state.messages.map((message) => (
              <article className={`message ${message.role}`} key={message.id}>
                <div className="message-role">{message.role}</div>
                <p>{message.content}</p>
              </article>
            ))
          )}
        </section>

        <form className="composer" onSubmit={submit}>
          <textarea
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            placeholder="Ask telos anything..."
            rows={3}
          />
          <button type="submit" disabled={!canSend}>
            Send
          </button>
        </form>
      </section>

      {settingsOpen && (
        <aside className="run-panel" aria-label="Run panel">
          <div className="run-panel-header">
            <div>
              <h2>Run panel</h2>
              <p>Settings and live activity</p>
            </div>
            <span className={`status-pill ${state.running ? "running" : ""}`}>
              {state.running ? "Running" : "Idle"}
            </span>
          </div>

          <section className="summary-grid" aria-label="Run summary">
            <div className="summary-tile">
              <span>Provider</span>
              <strong>{providerLabel}</strong>
            </div>
            <div className="summary-tile">
              <span>Approvals</span>
              <strong>{approvalLabel}</strong>
            </div>
            <div className="summary-tile wide">
              <span>Working directory</span>
              <strong title={cwdLabel}>{cwdLabel}</strong>
            </div>
          </section>

          <section className="panel-section">
            <div className="section-heading">
              <h3>Settings</h3>
              <span>{modelLabel}</span>
            </div>
            <label>
              Provider
              <select
                value={settings.provider}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    provider: event.target.value as ProviderKind,
                  }))
                }
              >
                <option value="mock">Mock</option>
                <option value="deepseek">DeepSeek</option>
              </select>
            </label>
            <label>
              API key
              <input
                type="password"
                value={settings.apiKey ?? ""}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, apiKey: event.target.value }))
                }
                placeholder="Required for DeepSeek"
              />
            </label>
            <label>
              Model
              <select
                value={settings.model ?? "auto"}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, model: event.target.value }))
                }
              >
                <option value="auto">Auto</option>
                <option value="deepseek-v4-pro">DeepSeek V4 Pro</option>
                <option value="deepseek-v4-flash">DeepSeek V4 Flash</option>
              </select>
            </label>
            <label>
              Working directory
              <input
                value={settings.cwd ?? ""}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, cwd: event.target.value }))
                }
                placeholder="Default: app launch directory"
              />
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.autoApprove}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    autoApprove: event.target.checked,
                  }))
                }
              />
              Auto-approve tools
            </label>
          </section>

          <section className="panel-section tool-activity">
            <div className="section-heading">
              <h3>Tool activity</h3>
              <span>{state.tools.length} recent</span>
            </div>
            {state.tools.length === 0 ? (
              <div className="empty-tools">
                Tool calls will appear here while telos works.
              </div>
            ) : (
              <div className="tool-list">
                {state.tools.map((tool) => (
                  <div className={`tool-item ${tool.status}`} key={tool.id}>
                    <div>
                      <strong>{tool.name}</strong>
                      <span>{tool.detail || tool.status}</span>
                    </div>
                    <em>{tool.status}</em>
                  </div>
                ))}
              </div>
            )}
          </section>
        </aside>
      )}
    </main>
  );
}

function normalizeSettings(settings: ChatSettings): ChatSettings {
  return {
    ...settings,
    apiKey: settings.apiKey?.trim() || undefined,
    cwd: settings.cwd?.trim() || undefined,
    model: settings.model?.trim() || "auto",
    maxIterations: settings.maxIterations ?? 30,
  };
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
