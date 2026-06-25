import { Activity, Bot } from "lucide-react";
import {
  DeepSeekBrowserPanel,
  DeepSeekExtractResult,
} from "@/components/DeepSeekBrowserPanel";
import { RunInspector } from "@/components/RunInspector";
import { ToolActivity } from "@/chatState";
import { SettingsSection } from "@/desktopTypes";
import { cn } from "@/lib/utils";
import { RunDisplay } from "@/runDisplay";

export type SideWorkspaceTab = "run" | "deepseek";

interface SideWorkspaceProps {
  activeTab: SideWorkspaceTab;
  display: RunDisplay;
  onChooseDirectory: () => void;
  onConfigure: (section: SettingsSection) => void;
  onOpenMemory: () => void;
  onSyncDeepSeek?: (result: DeepSeekExtractResult) => void;
  onTabChange: (tab: SideWorkspaceTab) => void;
  running: boolean;
  status: string;
  tools: ToolActivity[];
}

export function SideWorkspace({
  activeTab,
  display,
  onChooseDirectory,
  onConfigure,
  onOpenMemory,
  onSyncDeepSeek,
  onTabChange,
  running,
  status,
  tools,
}: SideWorkspaceProps) {
  return (
    <aside className="grid h-screen min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden border-l bg-muted max-[920px]:h-auto max-[920px]:min-h-0 max-[920px]:border-l-0 max-[920px]:border-t">
      <div className="border-b bg-background px-3 py-2">
        <div
          className="grid grid-cols-2 gap-1 rounded-md bg-muted p-1"
          role="tablist"
          aria-label="侧边工作区"
        >
          <WorkspaceTabButton
            active={activeTab === "run"}
            icon={<Activity className="size-3.5" />}
            label="运行"
            onClick={() => onTabChange("run")}
          />
          <WorkspaceTabButton
            active={activeTab === "deepseek"}
            icon={<Bot className="size-3.5" />}
            label="DeepSeek"
            onClick={() => onTabChange("deepseek")}
            attention={running}
          />
        </div>
      </div>

      <div className="min-h-0 min-w-0 overflow-hidden">
        {activeTab === "run" ? (
          <RunInspector
            display={display}
            onChooseDirectory={onChooseDirectory}
            onConfigure={onConfigure}
            onOpenMemory={onOpenMemory}
            running={running}
            status={status}
            tools={tools}
          />
        ) : (
          <DeepSeekBrowserPanel onSyncToAgent={onSyncDeepSeek} />
        )}
      </div>
    </aside>
  );
}

function WorkspaceTabButton({
  active,
  attention,
  icon,
  label,
  onClick,
}: {
  active: boolean;
  attention?: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={cn(
        "relative inline-flex min-h-8 items-center justify-center gap-1.5 rounded-sm px-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        active
          ? "bg-background text-foreground shadow-sm"
          : "text-muted-foreground hover:text-foreground",
      )}
      role="tab"
      aria-selected={active}
      onClick={onClick}
    >
      {icon}
      <span className="truncate">{label}</span>
      {attention && !active ? (
        <span className="absolute right-1.5 top-1.5 size-1.5 rounded-full bg-emerald-500" />
      ) : null}
    </button>
  );
}
