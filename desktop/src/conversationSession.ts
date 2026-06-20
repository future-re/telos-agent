import { ChatState, initialChatState } from "@/chatState";

const DEFAULT_TITLE = "新对话";
const MAX_TITLE_LENGTH = 32;

export interface ConversationSession {
  id: string;
  title: string;
  state: ChatState;
  createdAt: number;
  updatedAt: number;
}

export function createConversationSession(
  id: string,
  now = Date.now(),
): ConversationSession {
  return {
    id,
    title: DEFAULT_TITLE,
    state: initialChatState,
    createdAt: now,
    updatedAt: now,
  };
}

export function renameSessionFromPrompt(
  session: ConversationSession,
  prompt: string,
): ConversationSession {
  if (session.title !== DEFAULT_TITLE) {
    return session;
  }

  const title = compactTitle(prompt);
  return {
    ...session,
    title: title || DEFAULT_TITLE,
  };
}

export function updateSessionState(
  sessions: ConversationSession[],
  sessionId: string,
  update: (state: ChatState) => ChatState,
): ConversationSession[] {
  return sessions.map((session) => {
    if (session.id !== sessionId) {
      return session;
    }

    return {
      ...session,
      state: update(session.state),
      updatedAt: Date.now(),
    };
  });
}

export function deleteConversationSession(
  sessions: ConversationSession[],
  sessionId: string,
  activeSessionId: string,
  createFallback: () => ConversationSession = () =>
    createConversationSession(`session-${Date.now()}`),
): { sessions: ConversationSession[]; activeSessionId: string } {
  const deleteIndex = sessions.findIndex((session) => session.id === sessionId);
  if (deleteIndex === -1) {
    return { sessions, activeSessionId };
  }

  const remaining = sessions.filter((session) => session.id !== sessionId);
  if (remaining.length === 0) {
    const fallback = createFallback();
    return { sessions: [fallback], activeSessionId: fallback.id };
  }

  if (activeSessionId !== sessionId) {
    return { sessions: remaining, activeSessionId };
  }

  const nextSession = remaining[Math.min(deleteIndex, remaining.length - 1)];
  return { sessions: remaining, activeSessionId: nextSession.id };
}

function compactTitle(prompt: string): string {
  const compact = prompt.trim().replace(/\s+/g, " ");
  if (compact.length <= MAX_TITLE_LENGTH) {
    return compact;
  }
  return `${compact.slice(0, MAX_TITLE_LENGTH)}...`;
}
