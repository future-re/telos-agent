import { useEffect, useState } from "react";

const DOCK_MIN_WIDTH = 300;
const AGENT_RAIL_MIN_WIDTH = DOCK_MIN_WIDTH;
const AGENT_RAIL_MAX_WIDTH = 460;
const AGENT_RAIL_COLLAPSE_WIDTH = 96;
const DEFAULT_DOCK_WIDTH = 380;
const AGENT_RAIL_DEFAULT_WIDTH = DEFAULT_DOCK_WIDTH;
const SIDE_WORKSPACE_MIN_WIDTH = DOCK_MIN_WIDTH;
const SIDE_WORKSPACE_MAX_WIDTH = 760;
const SIDE_WORKSPACE_COLLAPSE_WIDTH = 120;
const SIDE_WORKSPACE_DEFAULT_WIDTH = DEFAULT_DOCK_WIDTH;

interface UseSideWorkspaceResizeOptions {
  onAgentRailCollapse?: () => void;
  onSideWorkspaceCollapse?: () => void;
}

export function useSideWorkspaceResize({
  onAgentRailCollapse,
  onSideWorkspaceCollapse,
}: UseSideWorkspaceResizeOptions = {}) {
  const [agentRailWidth, setAgentRailWidth] = useState(
    AGENT_RAIL_DEFAULT_WIDTH,
  );
  const [sideWorkspaceWidth, setSideWorkspaceWidth] = useState(
    SIDE_WORKSPACE_DEFAULT_WIDTH,
  );
  const [resizing, setResizing] = useState<"agent" | "side" | null>(null);

  useEffect(() => {
    if (!resizing) {
      return;
    }

    const handlePointerMove = (event: MouseEvent) => {
      if (resizing === "agent") {
        if (event.clientX <= AGENT_RAIL_COLLAPSE_WIDTH) {
          setResizing(null);
          onAgentRailCollapse?.();
          return;
        }

        setAgentRailWidth(clampAgentRailWidth(event.clientX));
        return;
      }

      const sideWidth = window.innerWidth - event.clientX;
      if (sideWidth <= SIDE_WORKSPACE_COLLAPSE_WIDTH) {
        setResizing(null);
        onSideWorkspaceCollapse?.();
        return;
      }

      setSideWorkspaceWidth(clampSideWorkspaceWidth(sideWidth));
    };
    const handlePointerUp = () => {
      setResizing(null);
    };

    window.addEventListener("mousemove", handlePointerMove);
    window.addEventListener("mouseup", handlePointerUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    return () => {
      window.removeEventListener("mousemove", handlePointerMove);
      window.removeEventListener("mouseup", handlePointerUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, [onAgentRailCollapse, onSideWorkspaceCollapse, resizing]);

  return {
    agentRailWidth,
    resetAgentRailWidth: () => setAgentRailWidth(AGENT_RAIL_DEFAULT_WIDTH),
    resetSideWorkspaceWidth: () =>
      setSideWorkspaceWidth(SIDE_WORKSPACE_DEFAULT_WIDTH),
    sideWorkspaceWidth,
    startAgentRailResize: () => setResizing("agent"),
    startSideWorkspaceResize: () => setResizing("side"),
  };
}

function clampAgentRailWidth(width: number): number {
  return Math.max(
    AGENT_RAIL_MIN_WIDTH,
    Math.min(AGENT_RAIL_MAX_WIDTH, Math.round(width)),
  );
}

function clampSideWorkspaceWidth(width: number): number {
  return Math.max(
    SIDE_WORKSPACE_MIN_WIDTH,
    Math.min(SIDE_WORKSPACE_MAX_WIDTH, Math.round(width)),
  );
}
