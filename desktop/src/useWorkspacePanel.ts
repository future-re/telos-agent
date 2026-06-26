import { useState } from "react";
import { SideWorkspaceTab } from "@/components/SideWorkspace";
import { SettingsSection } from "@/desktopTypes";
import { useSideWorkspaceResize } from "@/useSideWorkspaceResize";

export function useWorkspacePanel() {
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [sideWorkspaceTab, setSideWorkspaceTab] =
    useState<SideWorkspaceTab>("run");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("appearance");
  const { sideWorkspaceWidth, startSideWorkspaceResize } =
    useSideWorkspaceResize();

  function openSettings(section: SettingsSection) {
    setSettingsSection(section);
    setSettingsOpen(true);
    setInspectorOpen(true);
    setSideWorkspaceTab("run");
  }

  function openDeepSeekPanel() {
    setInspectorOpen(true);
    setSideWorkspaceTab("deepseek");
  }

  function toggleInspector() {
    setInspectorOpen((open) => !open);
  }

  return {
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
    startSideWorkspaceResize,
    toggleInspector,
  };
}
