import { invoke } from "@tauri-apps/api/core";
import { ChatMessage, ToolStatus } from "@/chatState";

export interface SessionSummary {
  id: string;
  title: string;
  messageCount: number;
  createdAtMs: number;
  updatedAtMs: number;
}

interface StoredBlock {
  Text?: { text: string };
  Thinking?: {
    text: string;
    signature?: string | null;
    is_redacted: boolean;
  };
  ToolCall?: { id: string; name: string; arguments: unknown };
  ToolResult?: {
    tool_call_id: string;
    name: string;
    content: unknown;
    is_error: boolean;
  };
}

type StoredRole = "System" | "User" | "Assistant" | "Tool";

interface StoredMessage {
  role: StoredRole;
  blocks: StoredBlock[];
}

const MAX_TITLE_LENGTH = 32;

export async function loadSessionList(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_sessions", {
    request: undefined,
  }).catch(() => []);
}

export async function loadSessionMessages(
  sessionId: string,
): Promise<ChatMessage[]> {
  const messages = await invoke<StoredMessage[]>("load_session", {
    request: { sessionId },
  }).catch(() => [] as StoredMessage[]);

  return storedMessagesToChatMessages(messages);
}

export function storedMessagesToChatMessages(
  messages: StoredMessage[],
): ChatMessage[] {
  const chatMessages: ChatMessage[] = [];
  let turnCounter = 0;
  let msgIdCounter = 0;

  for (const msg of messages) {
    if (msg.role === "System") {
      for (const block of msg.blocks) {
        const text = block.Text?.text ?? "";
        if (text.trim()) {
          chatMessages.push({
            id: `hist-${msgIdCounter++}`,
            role: "system",
            content: text,
          });
        }
      }
      continue;
    }

    if (msg.role === "User") {
      turnCounter++;
      const turnId = `turn-hist-${turnCounter}`;
      for (const block of msg.blocks) {
        const text = block.Text?.text ?? "";
        chatMessages.push({
          id: `hist-${msgIdCounter++}`,
          role: "user",
          content: text,
          turnId,
        });
      }
      continue;
    }

    if (msg.role === "Assistant") {
      const turnId = `turn-hist-${turnCounter}`;
      for (const block of msg.blocks) {
        if (block.Thinking) {
          chatMessages.push({
            id: `hist-${msgIdCounter++}`,
            role: "thinking",
            content: block.Thinking.text,
            turnId,
          });
        }
        if (block.ToolCall) {
          const args = summarizeArguments(block.ToolCall.arguments);
          chatMessages.push({
            id: `hist-${msgIdCounter++}`,
            role: "tool",
            content: `Running ${block.ToolCall.name}\n${args}`,
            turnId,
            toolName: block.ToolCall.name,
            toolDetail: args,
            toolStatus: "running" as ToolStatus,
          });
        }
        if (block.Text) {
          chatMessages.push({
            id: `hist-${msgIdCounter++}`,
            role: "assistant",
            content: block.Text.text,
            turnId,
          });
        }
      }
      continue;
    }

    if (msg.role === "Tool") {
      const turnId = `turn-hist-${turnCounter}`;
      for (const block of msg.blocks) {
        if (block.ToolResult) {
          const result = block.ToolResult;
          const formatted = formatToolResultMessage(
            result.name,
            result.is_error,
            result.content,
          );
          chatMessages.push({
            id: `hist-${msgIdCounter++}`,
            role: "tool",
            content: formatted,
            turnId,
            toolName: result.name,
            toolStatus: (result.is_error ? "failed" : "completed") as ToolStatus,
            isError: result.is_error,
            toolResultContent: result.content,
          });
        }
      }
    }
  }

  return chatMessages;
}

export function extractTitleFromPrompt(prompt: string): string {
  const compact = prompt.trim().replace(/\s+/g, " ");
  if (compact.length <= MAX_TITLE_LENGTH) return compact;
  return `${compact.slice(0, MAX_TITLE_LENGTH)}...`;
}

function summarizeArguments(args: unknown): string {
  try {
    if (args === null || args === undefined) return "";
    if (typeof args === "string") return args;
    return JSON.stringify(
      args,
      function replacer(this: Record<string, unknown>, key: string, value: unknown) {
        if (typeof value === "string" && value.length > 200) {
          return `${value.slice(0, 200)}...`;
        }
        return value;
      },
      2,
    );
  } catch {
    return String(args);
  }
}

function formatToolResultMessage(
  toolName: string,
  isError: boolean,
  content: unknown,
): string {
  const prefix = isError ? `Failed ${toolName}` : `Completed ${toolName}`;

  if (!content || typeof content !== "object") return prefix;

  const result = content as Record<string, unknown>;
  const normalizedName = toolName.trim().toLowerCase();

  if (
    (normalizedName === "bash" || normalizedName === "powershell") &&
    typeof result.stdout === "string"
  ) {
    const out = stripAnsi(result.stdout).trim();
    return out || typeof result.stderr === "string"
      ? stripAnsi(String(result.stderr)).trim() || prefix
      : prefix;
  }

  if (
    (normalizedName === "read" ||
      normalizedName === "write" ||
      normalizedName === "edit") &&
    typeof result.file_path === "string"
  ) {
    return String(result.file_path);
  }

  if (typeof result.message === "string") return result.message;
  if (typeof result.output === "string") return result.output;
  if (typeof result.content === "string") return result.content;

  return prefix;
}

function stripAnsi(value: string): string {
  return value
    .replace(/\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g, "")
    .replace(/\u001b(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])/g, "");
}
