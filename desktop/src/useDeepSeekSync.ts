import { useRef } from "react";
import { DeepSeekExtractResult } from "@/components/DeepSeekBrowserPanel";

export function useDeepSeekSync({
  appendSyncMessage,
  disabled,
}: {
  appendSyncMessage: (text: string) => void;
  disabled: boolean;
}) {
  const syncedContextRef = useRef("");

  function consumeSyncedContext() {
    const context = syncedContextRef.current;
    syncedContextRef.current = "";
    return context;
  }

  async function syncDeepSeek(result: DeepSeekExtractResult) {
    if (disabled) {
      return;
    }

    const text = (result.text ?? "").trim();
    if (!text) {
      return;
    }

    syncedContextRef.current = text;
    appendSyncMessage(text);
  }

  return {
    consumeSyncedContext,
    syncDeepSeek,
  };
}
