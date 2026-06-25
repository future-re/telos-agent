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

  it("does not create a duplicate tool entry for progress without toolCallId", () => {
    const started = reduceTelosEvent(initialChatState, {
      kind: "tool_call",
      toolCallId: "call-1",
      toolName: "PowerShell",
      detail: "Get-ChildItem",
    });
    const progressed = reduceTelosEvent(started, {
      kind: "tool_progress",
      message: "running PowerShell command with 120000ms timeout",
    });

    expect(progressed.tools).toHaveLength(1);
    expect(progressed.tools[0]).toMatchObject({
      id: "call-1",
      detail: "running PowerShell command with 120000ms timeout",
      status: "running",
    });
    expect(
      progressed.messages.filter((message) => message.role === "tool"),
    ).toHaveLength(1);
  });

  it("strips ansi escapes from PowerShell tool summaries", () => {
    const finished = reduceTelosEvent(initialChatState, {
      kind: "tool_result",
      toolCallId: "call-1",
      toolName: "PowerShell",
      toolResultContent: {
        stdout: "\u001b[32;1m名称\u001b[0m\r\n中文输出",
        stderr: "",
      },
    });

    expect(finished.messages[0]?.content).toBe("名称\r\n中文输出");
  });

  it("appends streaming tool output from progress events", () => {
    const started = reduceTelosEvent(initialChatState, {
      kind: "tool_call",
      toolCallId: "call-1",
      toolName: "PowerShell",
      detail: "Get-ChildItem",
    });
    const progressed = reduceTelosEvent(started, {
      kind: "tool_progress",
      toolCallId: "call-1",
      toolName: "PowerShell",
      message: "stdout update",
      data: { stream: "stdout", output: "line 1\n" },
    });

    expect(progressed.messages[0]).toMatchObject({
      role: "tool",
      toolName: "PowerShell",
      streaming: true,
    });
    expect(
      String(
        progressed.messages[0]?.toolResultContent &&
          (progressed.messages[0]?.toolResultContent as { stdout?: string })
            .stdout,
      ),
    ).toContain("line 1");
  });

  it("preserves status and success in tool result content but they can be hidden by the view", () => {
    const finished = reduceTelosEvent(initialChatState, {
      kind: "tool_result",
      toolCallId: "call-1",
      toolName: "PowerShell",
      toolResultContent: {
        status: 0,
        success: true,
        stdout: "done",
      },
    });

    expect(finished.messages[0]?.toolResultContent).toMatchObject({
      status: 0,
      success: true,
      stdout: "done",
    });
  });

  it("stores execution errors as tool result content for UI extraction", () => {
    const finished = reduceTelosEvent(initialChatState, {
      kind: "tool_result",
      toolCallId: "call-1",
      toolName: "PowerShell",
      isError: true,
      toolResultContent: {
        error: {
          kind: "execution_error",
          message: "tool PowerShell failed: ParserError",
        },
      },
    });

    expect(finished.messages[0]?.toolResultContent).toMatchObject({
      error: {
        kind: "execution_error",
        message: "tool PowerShell failed: ParserError",
      },
    });
  });
});
