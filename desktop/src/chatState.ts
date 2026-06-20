export type MessageRole = "user" | "assistant" | "thinking" | "system";

export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: string;
  streaming?: boolean;
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
}

export const initialChatState: ChatState = {
  messages: [],
  tools: [],
  status: "idle",
  running: false,
};

let nextMessageId = 1;

export function userMessage(content: string): ChatMessage {
  return {
    id: `local-${nextMessageId++}`,
    role: "user",
    content,
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
                status: event.isError ? "failed" : "completed",
                isError: Boolean(event.isError),
              }
            : tool,
        ),
      };
    case "turn_finished":
      return {
        ...state,
        status: "idle",
        running: false,
        messages: state.messages.map((message) => ({ ...message, streaming: false })),
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
  return {
    ...state,
    running: true,
    status: "thinking",
    messages: [...state.messages, userMessage(prompt)],
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
        streaming: true,
      },
    ],
  };
}

function upsertTool(tools: ToolActivity[], next: ToolActivity): ToolActivity[] {
  const index = tools.findIndex((tool) => tool.id === next.id);
  if (index === -1) {
    return [...tools, next];
  }
  return tools.map((tool, current) => (current === index ? next : tool));
}
