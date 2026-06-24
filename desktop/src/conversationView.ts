import { ChatMessage } from "@/chatState";

export type ConversationTurn =
  | {
      id: string;
      role: "user";
      turnId?: string;
      content: string;
      streaming?: boolean;
    }
  | {
      id: string;
      role: "system";
      turnId?: string;
      content: string;
      streaming?: boolean;
    }
  | {
      id: string;
      role: "tool";
      turnId?: string;
      content: string;
      streaming?: boolean;
      toolName?: string;
      toolStatus?: "running" | "completed" | "failed";
      isError?: boolean;
      toolDetail?: string;
      toolResultContent?: unknown;
    }
  | {
      id: string;
      role: "assistant";
      turnId?: string;
      content: string;
      thinking?: string;
      streaming?: boolean;
    };

export function groupConversationMessages(messages: ChatMessage[]): ConversationTurn[] {
  const turns: ConversationTurn[] = [];
  let pendingAssistant: Extract<ConversationTurn, { role: "assistant" }> | undefined;

  function flushAssistant() {
    if (pendingAssistant) {
      turns.push(pendingAssistant);
      pendingAssistant = undefined;
    }
  }

  for (const message of messages) {
    if (message.role === "thinking") {
      if (!pendingAssistant) {
        pendingAssistant = {
          id: `assistant-${message.id}`,
          role: "assistant",
          ...(message.turnId ? { turnId: message.turnId } : {}),
          content: "",
          thinking: message.content,
          streaming: message.streaming,
        };
      } else {
        pendingAssistant = {
          ...pendingAssistant,
          turnId: pendingAssistant.turnId ?? message.turnId,
          thinking: [pendingAssistant.thinking, message.content].filter(Boolean).join("\n"),
          streaming: pendingAssistant.streaming || message.streaming,
        };
      }
      continue;
    }

    if (message.role === "assistant") {
      if (!pendingAssistant) {
        pendingAssistant = {
          id: `assistant-${message.id}`,
          role: "assistant",
          ...(message.turnId ? { turnId: message.turnId } : {}),
          content: message.content,
          streaming: message.streaming,
        };
      } else {
        pendingAssistant = {
          ...pendingAssistant,
          turnId: pendingAssistant.turnId ?? message.turnId,
          content: [pendingAssistant.content, message.content].filter(Boolean).join("\n"),
          streaming: pendingAssistant.streaming || message.streaming,
        };
      }
      continue;
    }

    flushAssistant();
    turns.push({
      id: message.id,
      role: message.role,
      ...(message.turnId ? { turnId: message.turnId } : {}),
      content: message.content,
      streaming: message.streaming,
      ...(message.role === "tool"
        ? {
            toolName: message.toolName,
            toolStatus: message.toolStatus,
            isError: message.isError,
            toolDetail: message.toolDetail,
            toolResultContent: message.toolResultContent,
          }
        : {}),
    });
  }

  flushAssistant();
  return turns;
}
