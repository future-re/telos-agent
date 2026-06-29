import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TelosEvent } from "@/chatState";

export interface PendingApproval {
  sessionId: string;
  approvalId: string;
  toolCallId?: string;
  toolName: string;
  arguments: unknown;
  cwd?: string;
  reason?: string;
}

type ApprovalDecision = "allow" | "deny" | "modify";

export function useApprovals({
  activeSessionId,
  onResolved,
  onResolveError,
}: {
  activeSessionId: string;
  onResolved?: (approval: PendingApproval, decision: ApprovalDecision) => void;
  onResolveError: (sessionId: string, message: string) => void;
}) {
  const [pendingApprovals, setPendingApprovals] = useState<
    Record<string, PendingApproval>
  >({});
  const [approvalDraft, setApprovalDraft] = useState("");
  const [approvalError, setApprovalError] = useState("");
  const pendingApproval = pendingApprovals[activeSessionId];

  useEffect(() => {
    if (pendingApproval) {
      setApprovalDraft(formatJson(pendingApproval.arguments));
      setApprovalError("");
    }
  }, [pendingApproval?.approvalId]);

  function addApprovalFromEvent(sessionId: string, event: TelosEvent) {
    if (!event.approvalId) {
      return;
    }

    const nextApproval = {
      sessionId,
      approvalId: event.approvalId,
      toolCallId: event.toolCallId,
      toolName: event.toolName ?? "Tool",
      arguments: event.arguments ?? {},
      cwd: event.cwd,
      reason: event.reason,
    };
    setPendingApprovals((current) => ({
      ...current,
      [sessionId]: nextApproval,
    }));
    setApprovalError("");
  }

  function clearApproval(sessionId: string) {
    setPendingApprovals((current) => {
      const next = { ...current };
      delete next[sessionId];
      return next;
    });
    setApprovalError("");
  }

  function clearAllApprovals() {
    setPendingApprovals({});
    setApprovalError("");
  }

  async function resolveApproval(decision: ApprovalDecision) {
    if (!pendingApproval) {
      return;
    }

    let parsedArguments: unknown | undefined;
    if (decision === "modify") {
      try {
        parsedArguments = JSON.parse(approvalDraft);
      } catch (error) {
        setApprovalError(`JSON 无效：${String(error)}`);
        return;
      }
    }

    const approvalId = pendingApproval.approvalId;
    const resolvedApproval = pendingApproval;
    clearApproval(pendingApproval.sessionId);
    onResolved?.(resolvedApproval, decision);

    try {
      if (isTauriRuntime()) {
        await invoke("resolve_approval", {
          request: {
            sessionId: pendingApproval.sessionId,
            approvalId,
            decision,
            arguments: parsedArguments,
          },
        });
      }
    } catch (error) {
      onResolveError(
        pendingApproval.sessionId,
        `处理审批失败：${String(error)}`,
      );
    }
  }

  return {
    approvalDraft,
    approvalError,
    pendingApproval,
    addApprovalFromEvent,
    clearAllApprovals,
    clearApproval,
    resolveApproval,
    setApprovalDraft,
    setApprovalError,
  };
}

function formatJson(value: unknown): string {
  try {
    return JSON.stringify(value ?? {}, null, 2);
  } catch {
    return "{}";
  }
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
