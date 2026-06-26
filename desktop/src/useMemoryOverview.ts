import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MemoryOverview } from "@/desktopTypes";

export function useMemoryOverview({
  cwd,
  onError,
}: {
  cwd: string;
  onError: (message: string) => void;
}) {
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [memoryLoading, setMemoryLoading] = useState(false);
  const [memoryOverview, setMemoryOverview] = useState<
    MemoryOverview | undefined
  >();

  async function openMemoryOverview() {
    setMemoryOpen(true);
    setMemoryLoading(true);
    try {
      if (!isTauriRuntime()) {
        setMemoryOverview(browserPreviewMemoryOverview());
        return;
      }

      const overview = await invoke<MemoryOverview>("memory_summary", {
        request: cwd ? { cwd } : undefined,
      });
      setMemoryOverview(overview);
    } catch (error) {
      onError(`读取记忆失败：${String(error)}`);
      setMemoryOverview(undefined);
    } finally {
      setMemoryLoading(false);
    }
  }

  return {
    memoryLoading,
    memoryOpen,
    memoryOverview,
    openMemoryOverview,
    setMemoryOpen,
  };
}

function browserPreviewMemoryOverview(): MemoryOverview {
  return {
    root: "浏览器预览模式",
    total: 0,
    categories: [
      { label: "事实", count: 0 },
      { label: "命令", count: 0 },
      { label: "流程", count: 0 },
      { label: "模式", count: 0 },
      { label: "脚本", count: 0 },
    ],
    statuses: [
      { label: "可用", count: 0 },
      { label: "需确认", count: 0 },
      { label: "已废弃", count: 0 },
    ],
    recent: [],
  };
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
