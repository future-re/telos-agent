import { useState } from "react";
import { SideWorkspaceTab } from "@/components/SideWorkspace";
import { SettingsSection } from "@/desktopTypes";
import { useSideWorkspaceResize } from "@/useSideWorkspaceResize";

export function useWorkspacePanel() {
  const [agentRailOpen, setAgentRailOpen] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [sideWorkspaceTab, setSideWorkspaceTab] =
    useState<SideWorkspaceTab>("run");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("appearance");
  const {
    agentRailWidth,
    resetAgentRailWidth,
    resetSideWorkspaceWidth,
    sideWorkspaceWidth,
    startAgentRailResize,
    startSideWorkspaceResize,
  } = useSideWorkspaceResize({
    onAgentRailCollapse: () => setAgentRailOpen(false),
    onSideWorkspaceCollapse: () => setInspectorOpen(false),
  });

  function openSettings(section: SettingsSection) {
    setSettingsSection(section);
    setSettingsOpen(true);
    resetSideWorkspaceWidth();
    setInspectorOpen(true);
    setSideWorkspaceTab("run");
  }

  function openDeepSeekPanel() {
    resetSideWorkspaceWidth();
    setInspectorOpen(true);
    setSideWorkspaceTab("deepseek");
  }

  function toggleInspector() {
    setInspectorOpen((open) => {
      if (!open) {
        resetSideWorkspaceWidth();
      }
      return !open;
    });
  }

  function toggleAgentRail() {
    setAgentRailOpen((open) => {
      if (!open) {
        resetAgentRailWidth();
      }
      return !open;
    });
  }

  return {
    agentRailOpen,
    agentRailWidth,
    inspectorOpen,
    openDeepSeekPanel,
    openSettings,
    setSettingsOpen,
    setSettingsSection,
    setSideWorkspaceTab,
    settingsOpen,
    settingsSection,
    sideWorkspaceTab,
    sideWorkspaceWidth,
    startAgentRailResize,
    startSideWorkspaceResize,
    toggleAgentRail,
    toggleInspector,
  };
}
