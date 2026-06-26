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

export type ChatAction =
  | { type: "start"; prompt: string }
  | { type: "event"; event: TelosEvent }
  | { type: "error"; message: string }
  | { type: "reset" };

export function useConversationSessions(initialSessionId = "session-1") {
  const [sessions, setSessions] = useState<ConversationSession[]>([]);
  const [activeSessionId, setActiveSessionId] =
    useState(initialSessionId);
  const [sessionsReady, setSessionsReady] = useState(false);
  const loadedSessionsRef = useRef(new Set<string>());

  const activeSession = useMemo(
    () =>
      sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [activeSessionId, sessions],
  );
  const state = activeSession?.state ?? initialChatState;

  useEffect(() => {
    if (!isTauriRuntime()) {
      const session = createConversationSession(initialSessionId);
      setSessions([session]);
      setSessionsReady(true);
      return;
    }

    loadSessionList()
      .then((summaries) => {
        if (summaries.length > 0) {
          const loaded: ConversationSession[] = summaries.map((summary) => {
            const session = createConversationSession(
              summary.id,
              summary.createdAtMs,
            );
            return { ...session, title: summary.title };
          });
          setSessions(loaded);
          setActiveSessionId(loaded[0].id);

          loadSessionMessages(loaded[0].id)
            .then((chatMessages) => {
              loadedSessionsRef.current.add(loaded[0].id);
              setSessions((current) =>
                updateSessionState(
                  current,
                  loaded[0].id,
                  () => ({ ...initialChatState, messages: chatMessages }),
                ),
              );
            })
            .catch(() => undefined);
        } else {
          const session = createConversationSession(initialSessionId);
          setSessions([session]);
        }
        setSessionsReady(true);
      })
      .catch(() => {
        const session = createConversationSession(initialSessionId);
        setSessions([session]);
        setSessionsReady(true);
      });
  }, [initialSessionId]);

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
    sessionsReady,
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
