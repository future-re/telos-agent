import { describe, expect, it } from "vitest";
import { initialChatState, startUserTurn } from "@/chatState";
import {
  createConversationSession,
  deleteConversationSession,
  renameSessionFromPrompt,
  updateSessionState,
} from "@/conversationSession";

describe("conversation sessions", () => {
  it("keeps independent chat state for each conversation", () => {
    const first = createConversationSession("session-1");
    const second = createConversationSession("session-2");
    const updated = updateSessionState([first, second], "session-1", (state) =>
      startUserTurn(state, "重新设计桌面 UI"),
    );

    expect(updated.find((session) => session.id === "session-1")?.state.messages).toHaveLength(1);
    expect(updated.find((session) => session.id === "session-2")?.state).toEqual(initialChatState);
  });

  it("uses the first prompt as a compact conversation title", () => {
    const session = renameSessionFromPrompt(
      createConversationSession("session-1"),
      "请检查 desktop 的聊天面板布局是否会撑开",
    );

    expect(session.title).toBe("请检查 desktop 的聊天面板布局是否会撑开");
  });

  it("deletes a conversation and selects the next available conversation", () => {
    const first = createConversationSession("session-1", 1);
    const second = createConversationSession("session-2", 2);
    const third = createConversationSession("session-3", 3);

    const result = deleteConversationSession([first, second, third], "session-2", "session-2");

    expect(result.sessions.map((session) => session.id)).toEqual(["session-1", "session-3"]);
    expect(result.activeSessionId).toBe("session-3");
  });

  it("keeps one empty conversation when deleting the last conversation", () => {
    const result = deleteConversationSession(
      [createConversationSession("session-1", 1)],
      "session-1",
      "session-1",
      () => createConversationSession("session-new", 2),
    );

    expect(result.sessions).toHaveLength(1);
    expect(result.sessions[0].id).toBe("session-new");
    expect(result.activeSessionId).toBe("session-new");
  });
});
