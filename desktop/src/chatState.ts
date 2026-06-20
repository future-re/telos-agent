export type MessageRole = "user" | "assistant" | "thinking" | "system";

export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: string;
  turnId?: string;
  streaming?: boolean;
}

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  promptCacheHitTokens?: number;
  promptCacheMissTokens?: number;
  reasoningTokens?: number;
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
  text?: string;
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  promptCacheHitTokens?: number;
  promptCacheMissTokens?: number;
  reasoningTokens?: number;
  toolCallId?: string;
  toolName?: string;
  detail?: string;
  isError?: boolean;
  message?: string;
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
      return {
        ...state,
        status: event.detail || event.toolName || "运行工具",
        tools: upsertTool(state.tools, {
          id: event.toolCallId ?? `tool-${state.tools.length + 1}`,
          name: event.toolName ?? "Tool",
          detail: event.detail ?? "",
          status: "running",
          isError: false,
        }),
      };
    case "tool_progress":
      return {
        ...state,
        status: event.message ?? state.status,
        tools: state.tools.map((tool) =>
          tool.id === event.toolCallId
            ? { ...tool, detail: event.message ?? tool.detail }
            : tool,
        ),
      };
    case "tool_completed":
      return {
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
      };
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
    case "provider_retry":
    case "token_budget_exceeded":
      return {
        ...state,
        status: event.message ?? state.status,
      };
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

function applyProviderUsage(state: ChatState, event: TelosEvent): ChatState {
  if (event.inputTokens === undefined || event.outputTokens === undefined) {
    return state;
  }

  const nextUsage = addUsage(state.currentTurnUsage, {
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
  return {
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
