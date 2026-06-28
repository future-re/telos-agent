import {
  Bot,
  Check,
  Folder,
  Globe,
  KeyRound,
  Palette,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Plus,
  Settings,
  SlidersHorizontal,
  Sigma,
} from "lucide-react";
import { TokenUsage } from "@/chatState";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  AppearanceSettings,
  FontChoice,
  ThemeChoice,
  fontOptions,
  themeOptions,
} from "@/appearance";
import { SideWorkspaceTab } from "@/components/SideWorkspace";
import {
  DesktopSettingsOverrides,
  ResolvedDesktopSettings,
  SettingsSection,
} from "@/desktopTypes";
import { cn } from "@/lib/utils";
import { formatTokenCount } from "@/tokenUsage";
import { TokenUsageHistory } from "@/tokenUsageHistory";
import {
  buildTodayTokenMetrics,
  buildTokenUsageDashboard,
  TokenUsageDashboardItem,
} from "@/tokenUsageDashboard";

interface TopBarProps {
  agentRailOpen: boolean;
  metadata: string;
  panelOpen: boolean;
  settingsOpen: boolean;
  settingsSection: SettingsSection;
  settings: ResolvedDesktopSettings;
  appearance: AppearanceSettings;
  overrides: DesktopSettingsOverrides;
  apiKeyDraft: string;
  savingKey: boolean;
  sessionUsage?: TokenUsage;
  todayUsage?: TokenUsage;
  tokenHistory: TokenUsageHistory;
  onApiKeyDraftChange: (value: string) => void;
  onAppearanceChange: (settings: AppearanceSettings) => void;
  onOverridesChange: (settings: DesktopSettingsOverrides) => void;
  onSettingsOpenChange: (open: boolean) => void;
  onSettingsSectionChange: (section: SettingsSection) => void;
  onSaveApiKey: () => void;
  onOpenDeepSeek: () => void;
  onToggleAgentRail: () => void;
  onTogglePanel: () => void;
  onNewConversation: () => void;
  sideWorkspaceTab: SideWorkspaceTab;
  turnUsage?: TokenUsage;
  turnModel?: string | null;
}

export function TopBar({
  agentRailOpen,
  apiKeyDraft,
  appearance,
  metadata,
  onAppearanceChange,
  onApiKeyDraftChange,
  onOverridesChange,
  onNewConversation,
  onSaveApiKey,
  onSettingsOpenChange,
  onSettingsSectionChange,
  onToggleAgentRail,
  onTogglePanel,
  onOpenDeepSeek,
  overrides,
  panelOpen,
  savingKey,
  sessionUsage,
  settings,
  settingsOpen,
  settingsSection,
  sideWorkspaceTab,
  todayUsage,
  tokenHistory,
  turnUsage,
}: TopBarProps) {
  const mergedAutoApprove = overrides.autoApprove ?? settings.autoApprove;
  const mergedModel = normalizeModelValue(overrides.model ?? settings.model);
  const mergedCwd = overrides.cwd ?? settings.cwd;

  return (
    <header className="flex min-h-[4.25rem] w-full min-w-0 items-center justify-between gap-4 border-b bg-background/[0.92] px-4 py-3 backdrop-blur md:px-5">
      <div className="flex min-w-0 items-center gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-[0_8px_18px_rgba(23,32,44,0.18)]">
          <Bot className="size-4" aria-hidden="true" />
        </span>
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold leading-none">
            telos
          </h1>
          <p className="mt-1 truncate text-sm text-muted-foreground">
            {metadata}
          </p>
        </div>
      </div>

      <TokenTopStrip usage={todayUsage} />

      <div className="flex shrink-0 items-center gap-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label={agentRailOpen ? "隐藏左侧面板" : "显示左侧面板"}
              onClick={onToggleAgentRail}
            >
              {agentRailOpen ? (
                <PanelLeftClose className="size-4" aria-hidden="true" />
              ) : (
                <PanelLeftOpen className="size-4" aria-hidden="true" />
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {agentRailOpen ? "隐藏左侧面板" : "显示左侧面板"}
          </TooltipContent>
        </Tooltip>
        <SettingsDialog
          apiKeyDraft={apiKeyDraft}
          appearance={appearance}
          mergedAutoApprove={mergedAutoApprove}
          mergedCwd={mergedCwd}
          mergedModel={mergedModel}
          onApiKeyDraftChange={onApiKeyDraftChange}
          onAppearanceChange={onAppearanceChange}
          onOverridesChange={onOverridesChange}
          onOpenChange={onSettingsOpenChange}
          onSaveApiKey={onSaveApiKey}
          open={settingsOpen}
          overrides={overrides}
          savingKey={savingKey}
          sessionUsage={sessionUsage}
          settings={settings}
          tokenHistory={tokenHistory}
          activeSection={settingsSection}
          onActiveSectionChange={onSettingsSectionChange}
          turnUsage={turnUsage}
        />
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant={
                panelOpen && sideWorkspaceTab === "deepseek"
                  ? "default"
                  : "outline"
              }
              size="icon"
              aria-label="打开 DeepSeek 面板"
              onClick={onOpenDeepSeek}
            >
              <Globe className="size-4" aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>打开 DeepSeek 面板</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label={panelOpen ? "隐藏侧边栏" : "显示侧边栏"}
              onClick={onTogglePanel}
            >
              {panelOpen ? (
                <PanelRightClose className="size-4" aria-hidden="true" />
              ) : (
                <PanelRightOpen className="size-4" aria-hidden="true" />
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {panelOpen ? "隐藏侧边栏" : "显示侧边栏"}
          </TooltipContent>
        </Tooltip>
        <Button type="button" variant="outline" onClick={onNewConversation}>
          <Plus className="size-4" aria-hidden="true" />
          新对话
        </Button>
      </div>
    </header>
  );
}

function SettingsDialog({
  activeSection,
  apiKeyDraft,
  appearance,
  mergedAutoApprove,
  mergedCwd,
  mergedModel,
  onApiKeyDraftChange,
  onAppearanceChange,
  onOpenChange,
  onOverridesChange,
  onSaveApiKey,
  open,
  overrides,
  savingKey,
  sessionUsage,
  settings,
  tokenHistory,
  turnUsage,
  onActiveSectionChange,
}: {
  activeSection: SettingsSection;
  apiKeyDraft: string;
  appearance: AppearanceSettings;
  mergedAutoApprove: boolean;
  mergedCwd: string;
  mergedModel: string;
  onApiKeyDraftChange: (value: string) => void;
  onAppearanceChange: (settings: AppearanceSettings) => void;
  onOpenChange: (open: boolean) => void;
  onActiveSectionChange: (section: SettingsSection) => void;
  onOverridesChange: (settings: DesktopSettingsOverrides) => void;
  onSaveApiKey: () => void;
  open: boolean;
  overrides: DesktopSettingsOverrides;
  savingKey: boolean;
  sessionUsage?: TokenUsage;
  settings: ResolvedDesktopSettings;
  tokenHistory: TokenUsageHistory;
  turnUsage?: TokenUsage;
  turnModel?: string | null;
}) {
  const section = sectionMeta[activeSection] ?? sectionMeta.appearance;
  const usageDashboard = buildTokenUsageDashboard({ sessionUsage, turnUsage });
  const historyItems = Object.entries(tokenHistory)
    .sort(([left], [right]) => right.localeCompare(left))
    .slice(0, 14);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button
          type="button"
          variant={settings.apiKeyConfigured ? "outline" : "default"}
        >
          <Settings className="size-4" aria-hidden="true" />
          设置
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>运行设置</DialogTitle>
          <DialogDescription>
            桌面端读取 CLI
            配置、项目配置、记忆目录和工作目录；这里的改动作为当前桌面对话的覆盖项。
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-h-[360px] gap-4 md:grid-cols-[176px_minmax(0,1fr)]">
          <nav className="grid content-start gap-1" aria-label="设置分类">
            {settingsSections.map((item) => {
              const Icon = item.icon;
              const selected = activeSection === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    selected
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:bg-accent hover:text-foreground",
                  )}
                  aria-current={selected ? "page" : undefined}
                  onClick={() => onActiveSectionChange(item.id)}
                >
                  <Icon className="size-4 shrink-0" aria-hidden="true" />
                  <span className="truncate">{item.label}</span>
                </button>
              );
            })}
          </nav>

          <section className="min-w-0 rounded-md border bg-background p-4">
            <div className="mb-4">
              <h3 className="text-base font-semibold">{section.title}</h3>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                {section.description}
              </p>
            </div>

            {activeSection === "appearance" && (
              <div className="grid gap-4">
                <label className="grid gap-1.5">
                  <span className="text-sm font-medium">中文字体</span>
                  <Select
                    value={appearance.font}
                    onValueChange={(font) =>
                      onAppearanceChange({
                        ...appearance,
                        font: font as FontChoice,
                      })
                    }
                  >
                    <SelectTrigger aria-label="中文字体">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {fontOptions.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <span className="text-xs text-muted-foreground">
                    {
                      fontOptions.find(
                        (option) => option.value === appearance.font,
                      )?.description
                    }
                  </span>
                </label>

                <label className="grid gap-1.5">
                  <span className="text-sm font-medium">背景主题</span>
                  <Select
                    value={appearance.theme}
                    onValueChange={(theme) =>
                      onAppearanceChange({
                        ...appearance,
                        theme: theme as ThemeChoice,
                      })
                    }
                  >
                    <SelectTrigger aria-label="背景主题">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {themeOptions.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <div className="mt-1 flex gap-1.5" aria-hidden="true">
                    {themeOptions.map((option) => (
                      <span
                        key={option.value}
                        className={`size-5 rounded-md border shadow-sm theme-swatch-${option.value}`}
                      />
                    ))}
                  </div>
                </label>
              </div>
            )}

            {activeSection === "usage" && (
              <div className="grid gap-3">
                <div className="grid gap-2 sm:grid-cols-2">
                  {usageDashboard
                    .filter((item) => item.id !== "today")
                    .map((item) => (
                      <UsagePanelItem key={item.id} item={item} />
                    ))}
                </div>
                <div className="grid gap-2">
                  <div className="flex items-center justify-between gap-3">
                    <h4 className="text-sm font-semibold">历史统计</h4>
                    <span className="text-xs text-muted-foreground">
                      最近 14 天
                    </span>
                  </div>
                  <div className="grid max-h-52 gap-1.5 overflow-y-auto pr-2">
                    {historyItems.length === 0 ? (
                      <div className="rounded-md border border-dashed bg-muted/30 px-3 py-3 text-sm text-muted-foreground">
                        暂无历史 Token 上报。
                      </div>
                    ) : (
                      historyItems.map(([day, usage]) => (
                        <UsageHistoryRow key={day} day={day} usage={usage} />
                      ))
                    )}
                  </div>
                </div>
                <div className="rounded-md border bg-muted/30 px-3 py-2 text-xs leading-5 text-muted-foreground">
                  历史 Token 统计会保存在本机浏览器存储中。当前数据来自 provider
                  的真实 usage 上报；没有上报时不会估算。
                </div>
              </div>
            )}

            {activeSection === "service" && (
              <label className="grid gap-1.5">
                <span className="text-sm font-medium">服务提供方</span>
                <Select
                  value={overrides.provider ?? settings.provider}
                  onValueChange={(provider) =>
                    onOverridesChange({
                      ...overrides,
                      provider:
                        provider as DesktopSettingsOverrides["provider"],
                    })
                  }
                >
                  <SelectTrigger aria-label="服务提供方">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="deepseek">DeepSeek</SelectItem>
                    <SelectItem value="mock">Mock</SelectItem>
                  </SelectContent>
                </Select>
              </label>
            )}

            {activeSection === "key" && (
              <label className="grid gap-1.5">
                <span className="text-sm font-medium">DeepSeek API Key</span>
                <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                  <div className="relative min-w-0">
                    <KeyRound className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                    <Input
                      className="pl-9"
                      type="password"
                      value={apiKeyDraft}
                      onChange={(event) =>
                        onApiKeyDraftChange(event.target.value)
                      }
                      placeholder={
                        settings.apiKeyConfigured
                          ? "已写入 CLI 配置"
                          : "请输入 DeepSeek API Key"
                      }
                    />
                  </div>
                  <Button
                    type="button"
                    onClick={onSaveApiKey}
                    disabled={savingKey || !apiKeyDraft.trim()}
                  >
                    保存
                  </Button>
                </div>
                <span className="text-xs leading-5 text-muted-foreground">
                  保存到 {settings.configPath ?? "用户配置目录"}，CLI
                  和桌面端会共用这份配置。
                </span>
              </label>
            )}

            {activeSection === "approval" && (
              <div className="flex items-center justify-between gap-4 rounded-md border bg-muted/30 px-3 py-2.5">
                <div>
                  <span className="block text-sm font-medium">
                    自动批准工具
                  </span>
                  <span className="block text-xs text-muted-foreground">
                    对应 CLI 的 auto mode。
                  </span>
                </div>
                <Switch
                  checked={mergedAutoApprove}
                  onCheckedChange={(autoApprove) =>
                    onOverridesChange({ ...overrides, autoApprove })
                  }
                  aria-label="自动批准工具"
                />
              </div>
            )}

            {activeSection === "model" && (
              <label className="grid gap-1.5">
                <span className="text-sm font-medium">模型</span>
                <Select
                  value={mergedModel}
                  onValueChange={(model) =>
                    onOverridesChange({ ...overrides, model })
                  }
                >
                  <SelectTrigger aria-label="模型">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="flash">Flash</SelectItem>
                    <SelectItem value="pro">Pro</SelectItem>
                  </SelectContent>
                </Select>
              </label>
            )}

            {activeSection === "directory" && (
              <label className="grid gap-1.5">
                <span className="text-sm font-medium">工作目录</span>
                <div className="relative">
                  <Folder className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    className="pl-9"
                    value={mergedCwd}
                    onChange={(event) =>
                      onOverridesChange({
                        ...overrides,
                        cwd: event.target.value,
                      })
                    }
                  />
                </div>
              </label>
            )}
          </section>
        </div>

        <DialogFooter>
          <span className="mr-auto self-center text-xs text-muted-foreground">
            当前记忆：{settings.memoryCount} 条
          </span>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function TokenTopStrip({ usage }: { usage?: TokenUsage }) {
  const metrics = buildTodayTokenMetrics(usage);

  return (
    <div
      className="hidden min-w-0 flex-1 items-center justify-center px-2 xl:flex"
      aria-label="今日 Token 统计"
    >
      <span className="flex min-w-0 flex-wrap items-center justify-center gap-2 rounded-lg border bg-white/80 px-3 py-2 shadow-[0_6px_16px_rgba(15,23,42,0.045)]">
        <span className="shrink-0 text-xs font-semibold uppercase text-muted-foreground">
          Today
        </span>
        {metrics.map((item) => (
          <span
            key={item.id}
            className="whitespace-nowrap rounded-md bg-background px-2.5 py-1 text-[13px] text-muted-foreground"
          >
            {item.label}{" "}
            <strong className="font-mono text-sm font-semibold text-foreground">
              {item.value}
            </strong>
          </span>
        ))}
      </span>
    </div>
  );
}

const settingsSections: Array<{
  id: SettingsSection;
  label: string;
  icon: typeof Settings;
}> = [
  { id: "appearance", label: "界面外观", icon: Palette },
  { id: "usage", label: "Token 统计", icon: Sigma },
  { id: "service", label: "服务", icon: Bot },
  { id: "key", label: "密钥", icon: KeyRound },
  { id: "approval", label: "权限", icon: Check },
  { id: "model", label: "模型", icon: SlidersHorizontal },
  { id: "directory", label: "工作目录", icon: Folder },
];

const sectionMeta: Record<
  SettingsSection,
  { title: string; description: string }
> = {
  appearance: {
    title: "界面外观",
    description: "字体随桌面应用打包；背景主题只影响桌面端 UI。",
  },
  usage: {
    title: "Token 统计",
    description: "查看当前会话和当前单轮的真实 token 消耗。",
  },
  service: {
    title: "服务设置",
    description: "选择当前桌面对话使用的模型服务。",
  },
  key: {
    title: "密钥设置",
    description: "DeepSeek API Key 会写入 CLI 共用配置。",
  },
  approval: {
    title: "权限设置",
    description: "控制工具调用是否自动批准。",
  },
  model: {
    title: "模型设置",
    description: "选择 Flash 或 Pro。",
  },
  directory: {
    title: "工作目录",
    description: "覆盖当前桌面对话的运行目录。",
  },
};

function UsagePanelItem({ item }: { item: TokenUsageDashboardItem }) {
  return (
    <div className="rounded-md border bg-background px-3 py-2.5">
      <div className="flex items-center justify-between gap-3">
        <span className="text-sm font-medium text-muted-foreground">
          {item.label}
        </span>
        <strong
          className={cn(
            "font-mono text-base text-foreground",
            item.empty && "text-sm font-medium text-muted-foreground",
          )}
        >
          {item.value}
        </strong>
      </div>
      {item.details.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
          {item.details.map((detail) => (
            <span key={detail}>{detail}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function UsageHistoryRow({ day, usage }: { day: string; usage: TokenUsage }) {
  return (
    <div className="rounded-md border bg-background px-3 py-2">
      <div className="flex items-center justify-between gap-3">
        <strong className="font-mono text-sm text-foreground">{day}</strong>
        <strong className="font-mono text-sm text-foreground">
          {formatTokenCount(usage.totalTokens)}
        </strong>
      </div>
      <div className="mt-1.5 flex min-w-0 flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
        <span>输入 {formatTokenCount(usage.inputTokens)}</span>
        <span>输出 {formatTokenCount(usage.outputTokens)}</span>
        {usage.reasoningTokens !== undefined && (
          <span>思考 {formatTokenCount(usage.reasoningTokens)}</span>
        )}
      </div>
    </div>
  );
}

function normalizeModelValue(model?: string): string {
  switch (model?.trim().toLowerCase()) {
    case "pro":
    case "deepseek-v4-pro":
      return "pro";
    case "flash":
    case "deepseek-v4-flash":
    default:
      return "flash";
  }
}
