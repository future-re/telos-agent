import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  DesktopSettingsOverrides,
  ResolvedDesktopSettings,
} from "@/desktopTypes";

const fallbackSettings: ResolvedDesktopSettings = {
  provider: "deepseek",
  model: "auto",
  cwd: "",
  projectRootOrCwd: "",
  memoryRoot: "",
  memoryCount: 0,
  apiKeyConfigured: false,
  autoApprove: false,
  maxIterations: 30,
};

export function useDesktopSettings({
  onError,
  onResetSessions,
}: {
  onError: (message: string) => void;
  onResetSessions: () => void;
}) {
  const [settings, setSettings] =
    useState<ResolvedDesktopSettings>(fallbackSettings);
  const [overrides, setOverrides] = useState<DesktopSettingsOverrides>({});
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [loadingSettings, setLoadingSettings] = useState(true);

  const effectiveSettings = useMemo(
    () => ({
      ...settings,
      ...definedOverrides(overrides),
    }),
    [overrides, settings],
  );

  useEffect(() => {
    refreshSettings().catch((error) => {
      onError(`读取配置失败：${String(error)}`);
    });
  }, []);

  async function refreshSettings(nextOverrides = overrides) {
    if (!isTauriRuntime()) {
      setSettings({
        ...fallbackSettings,
        cwd: nextOverrides.cwd ?? "浏览器预览模式",
        projectRootOrCwd: nextOverrides.cwd ?? "浏览器预览模式",
      });
      setLoadingSettings(false);
      return;
    }

    const resolved = await invoke<ResolvedDesktopSettings>(
      "resolved_settings",
      {
        request: nextOverrides.cwd ? { cwd: nextOverrides.cwd } : undefined,
      },
    );
    setSettings(resolved);
    setLoadingSettings(false);
  }

  function updateOverrides(next: DesktopSettingsOverrides) {
    setOverrides(next);
    if (next.cwd !== overrides.cwd) {
      refreshSettings(next).catch((error) => {
        onError(`刷新配置失败：${String(error)}`);
      });
    }
  }

  async function saveApiKey() {
    const apiKey = apiKeyDraft.trim();
    if (!apiKey) {
      return;
    }
    setSavingKey(true);
    try {
      if (!isTauriRuntime()) {
        setSettings((current) => ({ ...current, apiKeyConfigured: true }));
        setApiKeyDraft("");
        return;
      }
      const resolved = await invoke<ResolvedDesktopSettings>(
        "save_deepseek_key",
        {
          request: { apiKey },
        },
      );
      setSettings(resolved);
      setOverrides((current) => ({
        ...current,
        provider: "deepseek",
        apiKey: undefined,
      }));
      setApiKeyDraft("");
      await invoke("reset_all_sessions").catch(() => undefined);
      onResetSessions();
    } catch (error) {
      onError(`保存 API Key 失败：${String(error)}`);
    } finally {
      setSavingKey(false);
    }
  }

  async function chooseDirectory() {
    if (!isTauriRuntime()) {
      const selected = window.prompt("输入工作目录", effectiveSettings.cwd);
      if (selected?.trim()) {
        const next = { ...overrides, cwd: selected.trim() };
        setOverrides(next);
        await refreshSettings(next);
      }
      return;
    }

    const selected = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: effectiveSettings.cwd || undefined,
      title: "选择工作目录",
    });
    if (typeof selected !== "string" || !selected.trim()) {
      return;
    }

    const next = { ...overrides, cwd: selected };
    setOverrides(next);
    await refreshSettings(next);
    await invoke("reset_all_sessions").catch(() => undefined);
    onResetSessions();
  }

  return {
    apiKeyDraft,
    chooseDirectory,
    effectiveSettings,
    loadingSettings,
    normalizeOverrides: () => normalizeOverrides(overrides, settings),
    overrides,
    saveApiKey,
    savingKey,
    setApiKeyDraft,
    settings,
    updateOverrides,
  };
}

function normalizeOverrides(
  overrides: DesktopSettingsOverrides,
  settings: ResolvedDesktopSettings,
): DesktopSettingsOverrides {
  return {
    provider: overrides.provider ?? settings.provider,
    apiKey: overrides.apiKey?.trim() || undefined,
    cwd: overrides.cwd?.trim() || settings.cwd || undefined,
    model: overrides.model?.trim() || settings.model || "auto",
    maxIterations: overrides.maxIterations ?? settings.maxIterations,
    autoApprove: overrides.autoApprove ?? settings.autoApprove,
  };
}

function definedOverrides(overrides: DesktopSettingsOverrides) {
  return Object.fromEntries(
    Object.entries(overrides).filter(
      ([, value]) => value !== undefined && value !== "",
    ),
  ) as Partial<ResolvedDesktopSettings>;
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
