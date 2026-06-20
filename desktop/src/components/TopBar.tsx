import { Bot, Folder, KeyRound, PanelRightClose, PanelRightOpen, Plus, Settings } from "lucide-react";
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
import { DesktopSettingsOverrides, ResolvedDesktopSettings } from "@/desktopTypes";

interface TopBarProps {
  metadata: string;
  panelOpen: boolean;
  settingsOpen: boolean;
  settings: ResolvedDesktopSettings;
  appearance: AppearanceSettings;
  overrides: DesktopSettingsOverrides;
  apiKeyDraft: string;
  savingKey: boolean;
  onApiKeyDraftChange: (value: string) => void;
  onAppearanceChange: (settings: AppearanceSettings) => void;
  onOverridesChange: (settings: DesktopSettingsOverrides) => void;
  onSettingsOpenChange: (open: boolean) => void;
  onSaveApiKey: () => void;
  onTogglePanel: () => void;
  onReset: () => void;
}

export function TopBar({
  apiKeyDraft,
  appearance,
  metadata,
  onAppearanceChange,
  onApiKeyDraftChange,
  onOverridesChange,
  onReset,
  onSaveApiKey,
  onSettingsOpenChange,
  onTogglePanel,
  overrides,
  panelOpen,
  savingKey,
  settings,
  settingsOpen,
}: TopBarProps) {
  const mergedAutoApprove = overrides.autoApprove ?? settings.autoApprove;
  const mergedModel = normalizeModelValue(overrides.model ?? settings.model);
  const mergedCwd = overrides.cwd ?? settings.cwd;

  return (
    <header className="flex min-h-16 w-full min-w-0 items-center justify-between gap-3 border-b bg-background/95 px-4 py-3 md:px-5">
      <div className="flex min-w-0 items-center gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground">
          <Bot className="size-4" aria-hidden="true" />
        </span>
        <div className="min-w-0">
          <h1 className="text-xl font-semibold leading-none tracking-normal">telos</h1>
          <p className="mt-1 truncate font-mono text-xs text-muted-foreground">{metadata}</p>
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-2">
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
          settings={settings}
        />
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label={panelOpen ? "隐藏运行状态" : "显示运行状态"}
              onClick={onTogglePanel}
            >
              {panelOpen ? (
                <PanelRightClose className="size-4" aria-hidden="true" />
              ) : (
                <PanelRightOpen className="size-4" aria-hidden="true" />
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{panelOpen ? "隐藏运行状态" : "显示运行状态"}</TooltipContent>
        </Tooltip>
        <Button type="button" variant="outline" onClick={onReset}>
          <Plus className="size-4" aria-hidden="true" />
          新对话
        </Button>
      </div>
    </header>
  );
}

function SettingsDialog({
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
  settings,
}: {
  apiKeyDraft: string;
  appearance: AppearanceSettings;
  mergedAutoApprove: boolean;
  mergedCwd: string;
  mergedModel: string;
  onApiKeyDraftChange: (value: string) => void;
  onAppearanceChange: (settings: AppearanceSettings) => void;
  onOpenChange: (open: boolean) => void;
  onOverridesChange: (settings: DesktopSettingsOverrides) => void;
  onSaveApiKey: () => void;
  open: boolean;
  overrides: DesktopSettingsOverrides;
  savingKey: boolean;
  settings: ResolvedDesktopSettings;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button type="button" variant={settings.apiKeyConfigured ? "outline" : "default"}>
          <Settings className="size-4" aria-hidden="true" />
          设置
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>运行设置</DialogTitle>
          <DialogDescription>
            桌面端读取 CLI 的配置文件、项目配置、记忆目录和工作目录；这里的改动只作为当前桌面对话的覆盖项。
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4">
          <section className="grid gap-3 rounded-md border bg-muted/25 p-3">
            <div>
              <h3 className="text-sm font-semibold">界面外观</h3>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">
                字体会随应用一起打包；背景主题只影响桌面端 UI，不写入 CLI agent 配置。
              </p>
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <label className="grid gap-1.5">
                <span className="text-sm font-medium">中文字体</span>
                <Select
                  value={appearance.font}
                  onValueChange={(font) =>
                    onAppearanceChange({ ...appearance, font: font as FontChoice })
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
                  {fontOptions.find((option) => option.value === appearance.font)?.description}
                </span>
              </label>

              <label className="grid gap-1.5">
                <span className="text-sm font-medium">背景主题</span>
                <Select
                  value={appearance.theme}
                  onValueChange={(theme) =>
                    onAppearanceChange({ ...appearance, theme: theme as ThemeChoice })
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
          </section>

          <label className="grid gap-1.5">
            <span className="text-sm font-medium">DeepSeek API Key</span>
            <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
              <div className="relative min-w-0">
                <KeyRound className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input
                  className="pl-9"
                  type="password"
                  value={apiKeyDraft}
                  onChange={(event) => onApiKeyDraftChange(event.target.value)}
                  placeholder={settings.apiKeyConfigured ? "已写入 CLI 配置" : "请输入 DeepSeek API Key"}
                />
              </div>
              <Button type="button" onClick={onSaveApiKey} disabled={savingKey || !apiKeyDraft.trim()}>
                保存
              </Button>
            </div>
            <span className="text-xs leading-5 text-muted-foreground">
              保存到 {settings.configPath ?? "用户配置目录"}，CLI 和桌面端会共用这份配置。
            </span>
          </label>

          <div className="grid gap-3 sm:grid-cols-2">
            <label className="grid gap-1.5">
              <span className="text-sm font-medium">模型</span>
              <Select
                value={mergedModel}
                onValueChange={(model) => onOverridesChange({ ...overrides, model })}
              >
                <SelectTrigger aria-label="模型">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="auto">自动</SelectItem>
                  <SelectItem value="pro">Pro</SelectItem>
                  <SelectItem value="flash">Flash</SelectItem>
                </SelectContent>
              </Select>
            </label>

            <div className="flex items-center justify-between gap-4 rounded-md border bg-muted/30 px-3 py-2.5">
              <div>
                <span className="block text-sm font-medium">自动批准工具</span>
                <span className="block text-xs text-muted-foreground">对应 CLI 的 auto mode。</span>
              </div>
              <Switch
                checked={mergedAutoApprove}
                onCheckedChange={(autoApprove) =>
                  onOverridesChange({ ...overrides, autoApprove })
                }
                aria-label="自动批准工具"
              />
            </div>
          </div>

          <label className="grid gap-1.5">
            <span className="text-sm font-medium">工作目录</span>
            <div className="relative">
              <Folder className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                className="pl-9"
                value={mergedCwd}
                onChange={(event) => onOverridesChange({ ...overrides, cwd: event.target.value })}
              />
            </div>
          </label>
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

function normalizeModelValue(model?: string): string {
  switch (model?.trim().toLowerCase()) {
    case "pro":
    case "deepseek-v4-pro":
      return "pro";
    case "flash":
    case "deepseek-v4-flash":
      return "flash";
    default:
      return "auto";
  }
}
