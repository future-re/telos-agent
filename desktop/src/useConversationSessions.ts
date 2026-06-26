import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ChatState,
  TelosEvent,
  initialChatState,
  reduceTelosEvent,
  startUserTurn,
} from "@/chatState";
import {
  ConversationSession,
  createConversationSession,
  deleteConversationSession,
  renameSessionFromPrompt,
  updateSessionState,
} from "@/conversationSession";
import { loadSessionList, loadSessionMessages } from "@/sessionLoader";
import { SessionSummary } from "@/sessionLoader";

export type ChatAction =
  | { type: "start"; prompt: string }
  | { type: "event"; event: TelosEvent }
  | { type: "error"; message: string }
  | { type: "reset" };

export function useConversationSessions(initialSessionId = "session-1") {
  const initialSession = useMemo(
    () => createConversationSession(initialSessionId),
    [initialSessionId],
  );
  const [sessions, setSessions] = useState<ConversationSession[]>([
    initialSession,
  ]);
  const [activeSessionId, setActiveSessionId] = useState(initialSession.id);
  const loadedSessionsRef = useRef(new Set<string>());
  const historyLoadedRef = useRef(false);

  const activeSession = useMemo(
    () =>
      sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [activeSessionId, sessions],
  );
  const state = activeSession?.state ?? initialChatState;

  useEffect(() => {
    if (!isTauriRuntime() || historyLoadedRef.current) {
      return;
    }
    historyLoadedRef.current = true;

    loadSessionList()
      .then((summaries) => {
        if (summaries.length === 0) return;

        const merged = mergeSessions(
          sessions,
          initialSessionId,
          summaries,
        );

        setSessions(merged);
      })
      .catch(() => undefined);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialSessionId]);

  function mergeSessions(
    current: ConversationSession[],
    defaultId: string,
    summaries: SessionSummary[],
  ): ConversationSession[] {
    const result: ConversationSession[] = [];
    const seen = new Set<string>();

    for (const summary of summaries) {
      const existing = current.find((s) => s.id === summary.id);
      if (existing) {
        result.push({ ...existing, title: summary.title });
      } else {
        const session = createConversationSession(
          summary.id,
          summary.createdAtMs,
        );
        result.push({ ...session, title: summary.title });
      }
      seen.add(summary.id);
    }

    for (const session of current) {
      if (!seen.has(session.id)) {
        result.push(session);
      }
    }

    return result;
  }

  const selectSession = useCallback(
    (sessionId: string) => {
      setActiveSessionId(sessionId);

      if (
        isTauriRuntime() &&
        !loadedSessionsRef.current.has(sessionId)
      ) {
        loadSessionMessages(sessionId)
          .then((chatMessages) => {
            loadedSessionsRef.current.add(sessionId);
            setSessions((current) =>
              updateSessionState(
                current,
                sessionId,
                () => ({ ...initialChatState, messages: chatMessages }),
              ),
            );
          })
          .catch(() => undefined);
      }
    },
    [],
  );

  function dispatchChatAction(
    action: ChatAction,
    sessionId = activeSessionId,
  ) {
    setSessions((current) =>
      updateSessionState(current, sessionId, (chatState) =>
        reduceChatAction(chatState, action),
      ),
    );
  }

  function applyTelosEvents(
    events: Array<{ sessionId: string; event: TelosEvent }>,
  ) {
    if (events.length === 0) {
      return;
    }

    setSessions((current) => {
      let next = current;
      for (const item of events) {
        next = updateSessionState(next, item.sessionId, (chatState) =>
          reduceChatAction(chatState, { type: "event", event: item.event }),
        );
      }
      return next;
    });
  }

  function startPrompt(prompt: string, sessionId = activeSessionId) {
    setSessions((current) =>
      updateSessionState(current, sessionId, (chatState) =>
        reduceChatAction(chatState, { type: "start", prompt }),
      ).map((session) =>
        session.id === sessionId
          ? renameSessionFromPrompt(session, prompt)
          : session,
      ),
    );
  }

  function resetAllSessionStates() {
    setSessions((current) =>
      current.map((session) => ({ ...session, state: initialChatState })),
    );
  }

  function createNewConversation() {
    const session = createConversationSession(`session-${Date.now()}`);
    setSessions((current) => [session, ...current]);
    setActiveSessionId(session.id);
    return session.id;
  }

  function deleteConversation(sessionId: string): boolean {
    const session = sessions.find((item) => item.id === sessionId);
    if (session?.state.running) {
      return false;
    }

    loadedSessionsRef.current.delete(sessionId);

    if (isTauriRuntime()) {
      invoke("reset_session", { request: { sessionId } }).catch(
        () => undefined,
      );
    }

    setSessions((current) => {
      const result = deleteConversationSession(
        current,
        sessionId,
        activeSessionId,
      );
      setActiveSessionId(result.activeSessionId);
      return result.sessions;
    });
    return true;
  }

  function appendDeepSeekSyncMessage(text: string, sessionId = activeSessionId) {
    const charCount = text.length;
    setSessions((current) =>
      updateSessionState(current, sessionId, (chatState) => ({
        ...chatState,
        messages: [
          ...chatState.messages,
          {
            id: `dsync-${Date.now()}`,
            role: "system",
            content: `从 DeepSeek 同步了 ${charCount} 个字符`,
            turnId: "deepseek-sync",
          },
          {
            id: `dsync-content-${Date.now()}`,
            role: "assistant",
            content: text,
            turnId: "deepseek-sync",
          },
        ],
      })),
    );
  }

  return {
    activeSession,
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
  };
}

function reduceChatAction(state: ChatState, action: ChatAction): ChatState {
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

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
