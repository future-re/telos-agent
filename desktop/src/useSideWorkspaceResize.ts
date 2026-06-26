import { useEffect, useState } from "react";

const SIDE_WORKSPACE_MIN_WIDTH = 360;
const SIDE_WORKSPACE_MAX_WIDTH = 760;
const SIDE_WORKSPACE_DEFAULT_WIDTH = 420;

export function useSideWorkspaceResize() {
  const [width, setWidth] = useState(SIDE_WORKSPACE_DEFAULT_WIDTH);
  const [resizing, setResizing] = useState(false);

  useEffect(() => {
    if (!resizing) {
      return;
    }

    const handlePointerMove = (event: MouseEvent) => {
      setWidth(clampSideWorkspaceWidth(window.innerWidth - event.clientX));
    };
    const handlePointerUp = () => {
      setResizing(false);
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
  }, [resizing]);

  return {
    sideWorkspaceWidth: width,
    startSideWorkspaceResize: () => setResizing(true),
  };
}

function clampSideWorkspaceWidth(width: number): number {
  return Math.max(
    SIDE_WORKSPACE_MIN_WIDTH,
    Math.min(SIDE_WORKSPACE_MAX_WIDTH, Math.round(width)),
  );
}
