import { FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChatAction } from "@/useConversationSessions";
import { DesktopSettingsOverrides } from "@/desktopTypes";

interface PromptResult {
  finalText: string;
}

export function useAgentCommands({
  activeSessionId,
  clearApproval,
  consumeSyncedContext,
  createNewConversation,
  deepseekNeedsKey,
  deleteConversation,
  dispatch,
  normalizeOverrides,
  prompt,
  running,
  setPrompt,
  startPrompt,
}: {
  activeSessionId: string;
  clearApproval: (sessionId: string) => void;
  consumeSyncedContext: () => string;
  createNewConversation: () => string;
  deepseekNeedsKey: boolean;
  deleteConversation: (sessionId: string) => boolean;
  dispatch: (action: ChatAction, sessionId?: string) => void;
  normalizeOverrides: () => DesktopSettingsOverrides;
  prompt: string;
  running: boolean;
  setPrompt: (prompt: string) => void;
  startPrompt: (prompt: string, sessionId?: string) => void;
}) {
  async function submit(event: FormEvent) {
    event.preventDefault();
    const text = prompt.trim();
    if (!text || running || deepseekNeedsKey) {
      return;
    }
    setPrompt("");

    const pendingContext = consumeSyncedContext();
    const fullPrompt = pendingContext
      ? `以下是从 DeepSeek 同步的上下文，请在回答时参考：\n\n${pendingContext}\n\n---\n用户消息：${text}`
      : text;

    startPrompt(fullPrompt);
    try {
      await invoke<PromptResult>("send_prompt", {
        request: {
          sessionId: activeSessionId,
          prompt: fullPrompt,
          settings: normalizeOverrides(),
        },
      });
    } catch (error) {
      dispatch({ type: "error", message: String(error) });
    }
  }

  async function stopCurrentTask() {
    if (isTauriRuntime()) {
      await invoke("cancel_current_task", {
        request: { sessionId: activeSessionId },
      }).catch((error) => {
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
      activeSessionId,
    );
    clearApproval(activeSessionId);
  }

  function startNewConversation() {
    createNewConversation();
    setPrompt("");
  }

  function removeConversation(sessionId: string) {
    if (!deleteConversation(sessionId)) {
      return;
    }
    clearApproval(sessionId);
  }

  return {
    removeConversation,
    startNewConversation,
    stopCurrentTask,
    submit,
  };
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
