export type MessageRole = "user" | "assistant" | "thinking" | "system" | "tool";

export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: string;
  turnId?: string;
  streaming?: boolean;
  toolName?: string;
  toolStatus?: ToolStatus;
  isError?: boolean;
  toolDetail?: string;
  toolResultContent?: unknown;
}

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  promptCacheHitTokens?: number;
  promptCacheMissTokens?: number;
  reasoningTokens?: number;
  model?: string;
}

export type ToolStatus = "running" | "completed" | "failed";

export interface ToolActivity {
  id: string;
  name: string;
  detail: string;
  status: ToolStatus;
  isError: boolean;
}

export interface TelosEvent {
  kind: string;
  sessionId?: string;
  text?: string;
  approvalId?: string;
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  promptCacheHitTokens?: number;
  promptCacheMissTokens?: number;
  reasoningTokens?: number;
  model?: string;
  toolCallId?: string;
  toolName?: string;
  arguments?: unknown;
  cwd?: string;
  reason?: string;
  detail?: string;
  isError?: boolean;
  message?: string;
  toolResultContent?: unknown;
}

export interface ChatState {
  messages: ChatMessage[];
  tools: ToolActivity[];
  status: string;
  running: boolean;
  currentTurnId?: string;
  currentTurnUsage?: TokenUsage;
  usageByTurnId: Record<string, TokenUsage>;
}

export const initialChatState: ChatState = {
  messages: [],
  tools: [],
  status: "idle",
  running: false,
  usageByTurnId: {},
};

let nextMessageId = 1;

export function userMessage(content: string, turnId?: string): ChatMessage {
  return {
    id: turnId ?? `local-${nextMessageId++}`,
    role: "user",
    content,
    turnId,
  };
}

export function reduceTelosEvent(state: ChatState, event: TelosEvent): ChatState {
  switch (event.kind) {
    case "assistant_delta":
      return appendStreamingMessage(state, "assistant", event.text ?? "", "streaming");
    case "thinking_delta":
      return appendStreamingMessage(state, "thinking", event.text ?? "", "thinking");
    case "tool_call":
      return appendOrUpdateToolMessage(
        {
          ...state,
          status: event.detail || event.toolName || "运行工具",
          tools: upsertTool(state.tools, {
            id: event.toolCallId ?? `tool-${state.tools.length + 1}`,
            name: event.toolName ?? "Tool",
            detail: event.detail ?? "",
            status: "running",
            isError: false,
          }),
        },
        event.toolCallId ?? `tool-${state.tools.length + 1}`,
        event.toolName ?? "Tool",
        event.detail ?? "",
        "running",
        false,
        undefined,
      );
    case "tool_progress":
      return appendOrUpdateToolMessage(
        {
          ...state,
          status: event.message ?? state.status,
          tools: state.tools.map((tool) =>
            tool.id === event.toolCallId
              ? { ...tool, detail: event.message ?? tool.detail }
              : tool,
          ),
        },
        event.toolCallId ?? `tool-${state.tools.length + 1}`,
        event.toolName ??
          state.tools.find((tool) => tool.id === event.toolCallId)?.name ??
          "Tool",
        event.message ?? state.tools.find((tool) => tool.id === event.toolCallId)?.detail ?? "",
        "running",
        false,
        state.messages.find((message) => message.id === `tool-message-${event.toolCallId}`)?.toolResultContent,
      );
    case "tool_completed":
      return appendOrUpdateToolMessage(
        {
          ...state,
          status: event.isError ? "tool failed" : "tool completed",
          tools: state.tools.map((tool) =>
            tool.id === event.toolCallId
              ? {
                  ...tool,
                  name: event.toolName ?? tool.name,
                  detail: event.detail ?? tool.detail,
                  status: event.isError ? "failed" : "completed",
                  isError: Boolean(event.isError),
                }
              : tool,
          ),
        },
        event.toolCallId ?? `tool-${state.tools.length + 1}`,
        event.toolName ??
          state.tools.find((tool) => tool.id === event.toolCallId)?.name ??
          "Tool",
        event.detail ?? state.tools.find((tool) => tool.id === event.toolCallId)?.detail ?? "",
        event.isError ? "failed" : "completed",
        Boolean(event.isError),
        state.messages.find((message) => message.id === `tool-message-${event.toolCallId}`)?.toolResultContent,
      );
    case "tool_result":
      return appendOrUpdateToolMessage(
        state,
        event.toolCallId ?? `tool-${state.tools.length + 1}`,
        event.toolName ??
          state.tools.find((tool) => tool.id === event.toolCallId)?.name ??
          "Tool",
        state.tools.find((tool) => tool.id === event.toolCallId)?.detail ?? "",
        state.messages.find((message) => message.id === `tool-message-${event.toolCallId}`)?.toolStatus ??
          state.tools.find((tool) => tool.id === event.toolCallId)?.status ??
          "running",
        Boolean(event.isError),
        event.toolResultContent,
      );
    case "provider_usage":
      return applyProviderUsage(state, event);
    case "turn_finished":
      return {
        ...state,
        status: "idle",
        running: false,
        messages: state.messages.map((message) => ({ ...message, streaming: false })),
        usageByTurnId:
          state.currentTurnId && state.currentTurnUsage
            ? { ...state.usageByTurnId, [state.currentTurnId]: state.currentTurnUsage }
            : state.usageByTurnId,
      };
    case "cancelled":
      return {
        ...state,
        status: "idle",
        running: false,
        messages: state.messages.map((message) => ({ ...message, streaming: false })),
        tools: state.tools.map((tool) =>
          tool.status === "running" ? { ...tool, status: "failed", detail: "已停止" } : tool,
        ),
      };
    case "approval_required":
    case "approval_requested":
      return {
        ...state,
        status: event.reason ?? event.message ?? "等待工具审批",
      };
    case "approval_resolved":
      return {
        ...state,
        status: event.message ?? "审批已处理",
      };
    case "provider_retry":
      return appendSystemEvent(state, event.message ?? "provider retry");
    case "token_budget_exceeded":
      return appendSystemEvent(state, event.message ?? "token budget exceeded");
    default:
      return state;
  }
}

export function startUserTurn(state: ChatState, prompt: string): ChatState {
  const turnId = `turn-${nextMessageId++}`;
  return {
    ...state,
    running: true,
    status: "thinking",
    currentTurnId: turnId,
    currentTurnUsage: undefined,
    messages: [...state.messages, userMessage(prompt, turnId)],
    tools: [],
  };
}

function appendStreamingMessage(
  state: ChatState,
  role: MessageRole,
  text: string,
  status: string,
): ChatState {
  if (!text) {
    return { ...state, status, running: true };
  }

  const last = state.messages[state.messages.length - 1];
  if (last?.role === role && last.streaming) {
    return {
      ...state,
      status,
      running: true,
      messages: [
        ...state.messages.slice(0, -1),
        { ...last, content: last.content + text },
      ],
    };
  }

  return {
    ...state,
    status,
    running: true,
    messages: [
      ...state.messages,
      {
        id: `event-${nextMessageId++}`,
        role,
        content: text,
        turnId: state.currentTurnId,
        streaming: true,
      },
    ],
  };
}

function appendOrUpdateToolMessage(
  state: ChatState,
  toolCallId: string,
  toolName: string,
  detail: string,
  toolStatus: ToolStatus,
  isError: boolean,
  toolResultContent: unknown,
): ChatState {
  const messageId = `tool-message-${toolCallId}`;
  const summary = formatToolMessage(toolName, detail, toolStatus, toolResultContent);
  const existingIndex = state.messages.findIndex((message) => message.id === messageId);
  const nextMessage: ChatMessage = {
    id: messageId,
    role: "tool",
    content: summary,
    turnId: state.currentTurnId,
    streaming: toolStatus === "running",
    toolName,
    toolStatus,
    isError,
    toolDetail: detail,
    toolResultContent,
  };

  if (existingIndex === -1) {
    return {
      ...state,
      messages: [...state.messages, nextMessage],
    };
  }

  return {
    ...state,
    messages: state.messages.map((message, index) =>
      index === existingIndex ? { ...message, ...nextMessage } : message,
    ),
  };
}

function appendSystemEvent(state: ChatState, content: string): ChatState {
  if (!content.trim()) {
    return state;
  }

  return {
    ...state,
    status: content,
    messages: [
      ...state.messages,
      {
        id: `system-${nextMessageId++}`,
        role: "system",
        content,
        turnId: state.currentTurnId,
      },
    ],
  };
}

function applyProviderUsage(state: ChatState, event: TelosEvent): ChatState {
  if (event.inputTokens === undefined || event.outputTokens === undefined) {
    return state;
  }

  const nextUsage = addUsage(state.currentTurnUsage, {
    model: event.model,
    inputTokens: event.inputTokens,
    outputTokens: event.outputTokens,
    totalTokens: event.totalTokens ?? event.inputTokens + event.outputTokens,
    promptCacheHitTokens: event.promptCacheHitTokens,
    promptCacheMissTokens: event.promptCacheMissTokens,
    reasoningTokens: event.reasoningTokens,
  });

  return {
    ...state,
    currentTurnUsage: nextUsage,
    usageByTurnId: state.currentTurnId
      ? { ...state.usageByTurnId, [state.currentTurnId]: nextUsage }
      : state.usageByTurnId,
  };
}

function addUsage(current: TokenUsage | undefined, next: TokenUsage): TokenUsage {
  const model = next.model ?? current?.model;
  return {
    model,
    inputTokens: (current?.inputTokens ?? 0) + next.inputTokens,
    outputTokens: (current?.outputTokens ?? 0) + next.outputTokens,
    totalTokens: (current?.totalTokens ?? 0) + next.totalTokens,
    promptCacheHitTokens: addOptional(current?.promptCacheHitTokens, next.promptCacheHitTokens),
    promptCacheMissTokens: addOptional(current?.promptCacheMissTokens, next.promptCacheMissTokens),
    reasoningTokens: addOptional(current?.reasoningTokens, next.reasoningTokens),
  };
}

function addOptional(current: number | undefined, next: number | undefined): number | undefined {
  if (current === undefined && next === undefined) {
    return undefined;
  }
  return (current ?? 0) + (next ?? 0);
}

function upsertTool(tools: ToolActivity[], next: ToolActivity): ToolActivity[] {
  const index = tools.findIndex((tool) => tool.id === next.id);
  if (index === -1) {
    return [...tools, next];
  }
  return tools.map((tool, current) => (current === index ? next : tool));
}

function formatToolMessage(
  toolName: string,
  detail: string,
  status: ToolStatus,
  toolResultContent: unknown,
): string {
  const renderedResult = summarizeToolResult(toolName, toolResultContent);
  if (renderedResult) {
    return renderedResult;
  }
  const title =
    status === "completed"
      ? `Completed ${toolName}`
      : status === "failed"
        ? `Failed ${toolName}`
        : `Running ${toolName}`;
  const trimmed = detail.trim();
  return trimmed ? `${title}\n${trimmed}` : title;
}

function summarizeToolResult(toolName: string, toolResultContent: unknown): string | undefined {
  if (!toolResultContent || typeof toolResultContent !== "object") {
    return undefined;
  }

  const result = toolResultContent as Record<string, unknown>;
  const normalizedName = toolName.trim().toLowerCase();

  if ((normalizedName === "bash" || normalizedName === "powershell") && typeof result.stdout === "string") {
    return result.stdout.trim() || (typeof result.stderr === "string" ? result.stderr.trim() : "");
  }

  if ((normalizedName === "read" || normalizedName === "write" || normalizedName === "edit") && typeof result.file_path === "string") {
    return String(result.file_path);
  }

  if (typeof result.message === "string") {
    return result.message;
  }
  if (typeof result.output === "string") {
    return result.output;
  }
  if (typeof result.content === "string") {
    return result.content;
  }

  return undefined;
}
