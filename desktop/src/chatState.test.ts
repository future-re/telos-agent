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

  it("keeps tool failure details from completion events", () => {
    const started = reduceTelosEvent(initialChatState, {
      kind: "tool_call",
      toolCallId: "call-1",
      toolName: "Bash",
      detail: "npm run test",
    });
    const finished = reduceTelosEvent(started, {
      kind: "tool_completed",
      toolCallId: "call-1",
      toolName: "Bash",
      detail: "exit code 1\nTest suite failed",
      isError: true,
    });

    expect(finished.tools[0]).toMatchObject({
      detail: "exit code 1\nTest suite failed",
      status: "failed",
      isError: true,
    });
  });

  it("aggregates provider token usage for the active turn", () => {
    const started = reduceTelosEvent(initialChatState, {
      kind: "provider_usage",
      inputTokens: 10,
      outputTokens: 4,
      totalTokens: 14,
      reasoningTokens: 2,
    });
    const updated = reduceTelosEvent(started, {
      kind: "provider_usage",
      inputTokens: 6,
      outputTokens: 3,
      totalTokens: 9,
    });

    expect(updated.currentTurnUsage).toEqual({
      inputTokens: 16,
      outputTokens: 7,
      totalTokens: 23,
      reasoningTokens: 2,
    });
  });
});
