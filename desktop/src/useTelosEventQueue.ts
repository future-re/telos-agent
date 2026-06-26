import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { TelosEvent } from "@/chatState";

type QueuedTelosEvent = {
  sessionId: string;
  event: TelosEvent;
};

export function useTelosEventQueue({
  activeSessionId,
  onApprovalRequired,
  onEvents,
  onProviderUsage,
}: {
  activeSessionId: string;
  onApprovalRequired: (sessionId: string, event: TelosEvent) => void;
  onEvents: (events: QueuedTelosEvent[]) => void;
  onProviderUsage: (event: TelosEvent) => void;
}) {
  const activeSessionIdRef = useRef(activeSessionId);
  const eventQueueRef = useRef<QueuedTelosEvent[]>([]);
  const eventFrameRef = useRef<number | null>(null);
  const onApprovalRequiredRef = useRef(onApprovalRequired);
  const onEventsRef = useRef(onEvents);
  const onProviderUsageRef = useRef(onProviderUsage);

  useEffect(() => {
    activeSessionIdRef.current = activeSessionId;
    onApprovalRequiredRef.current = onApprovalRequired;
    onEventsRef.current = onEvents;
    onProviderUsageRef.current = onProviderUsage;
  }, [activeSessionId, onApprovalRequired, onEvents, onProviderUsage]);

  useEffect(() => {
    const flushQueuedEvents = () => {
      eventFrameRef.current = null;
      const queued = eventQueueRef.current;
      if (queued.length === 0) {
        return;
      }
      eventQueueRef.current = [];
      onEventsRef.current(queued);
    };

    const unlisten = listen<TelosEvent>("telos://event", (event) => {
      const payload = event.payload;
      const targetSessionId = payload.sessionId ?? activeSessionIdRef.current;

      if (payload.kind === "approval_required" && payload.approvalId) {
        onApprovalRequiredRef.current(targetSessionId, payload);
      }
      if (payload.kind === "provider_usage") {
        onProviderUsageRef.current(payload);
      }

      eventQueueRef.current.push({
        sessionId: targetSessionId,
        event: payload,
      });

      if (eventFrameRef.current === null) {
        eventFrameRef.current = window.requestAnimationFrame(flushQueuedEvents);
      }
    });

    return () => {
      if (eventFrameRef.current !== null) {
        window.cancelAnimationFrame(eventFrameRef.current);
        eventFrameRef.current = null;
      }
      unlisten.then((fn) => fn());
    };
  }, []);
}
