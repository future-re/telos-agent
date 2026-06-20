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
          <div>
            <h1>telos</h1>
            <span>{state.status}</span>
          </div>
          <div className="topbar-actions">
            <button type="button" onClick={() => setSettingsOpen((open) => !open)}>
              Settings
            </button>
            <button type="button" onClick={resetSession}>
              New Chat
            </button>
          </div>
        </header>

        <section className="conversation" aria-live="polite">
          {state.messages.length === 0 ? (
            <div className="empty-state">
              <h2>Start a conversation</h2>
              <p>Ask telos to answer questions, inspect files, or run approved tools.</p>
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

        {state.tools.length > 0 && (
          <aside className="tool-strip">
            {state.tools.map((tool) => (
              <div className={`tool-item ${tool.status}`} key={tool.id}>
                <strong>{tool.name}</strong>
                <span>{tool.detail || tool.status}</span>
              </div>
            ))}
          </aside>
        )}

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
        <aside className="settings-panel">
          <h2>Settings</h2>
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
