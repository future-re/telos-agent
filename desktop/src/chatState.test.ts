import { describe, expect, it } from "vitest";
import { initialChatState, reduceTelosEvent } from "./chatState";

describe("reduceTelosEvent", () => {
  it("appends assistant deltas into a streaming assistant message", () => {
    const first = reduceTelosEvent(initialChatState, {
      kind: "assistant_delta",
      text: "hel",
    });
    const second = reduceTelosEvent(first, {
      kind: "assistant_delta",
      text: "lo",
    });

    expect(second.messages).toHaveLength(1);
    expect(second.messages[0]).toMatchObject({
      role: "assistant",
      content: "hello",
      streaming: true,
    });
  });

  it("records tool calls and marks completion", () => {
    const started = reduceTelosEvent(initialChatState, {
      kind: "tool_call",
      toolCallId: "call-1",
      toolName: "Bash",
      detail: "ls",
    });
    const finished = reduceTelosEvent(started, {
      kind: "tool_completed",
      toolCallId: "call-1",
      toolName: "Bash",
      isError: false,
    });

    expect(finished.tools).toEqual([
      {
        id: "call-1",
        name: "Bash",
        detail: "ls",
        status: "completed",
        isError: false,
      },
    ]);
  });
});
