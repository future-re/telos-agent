import { useMemo, useState } from "react";
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
  const activeSession = useMemo(
    () =>
      sessions.find((session) => session.id === activeSessionId) ?? sessions[0],
    [activeSessionId, sessions],
  );
  const state = activeSession?.state ?? initialChatState;

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
        session.id === sessionId ? renameSessionFromPrompt(session, prompt) : session,
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

    setSessions((current) => {
      const result = deleteConversationSession(current, sessionId, activeSessionId);
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
    selectSession: setActiveSessionId,
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
