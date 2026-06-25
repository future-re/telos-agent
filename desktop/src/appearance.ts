export type FontChoice = "noto-sans" | "system" | "wenkai" | "serif";
export type ThemeChoice = "light" | "warm" | "ink" | "green";

export interface AppearanceSettings {
  font: FontChoice;
  theme: ThemeChoice;
}

const STORAGE_KEY = "telos.desktop.appearance";

export const defaultAppearance: AppearanceSettings = {
  font: "noto-sans",
  theme: "light",
};

export const fontOptions: Array<{
  value: FontChoice;
  label: string;
  description: string;
}> = [
  { value: "noto-sans", label: "Noto Sans SC", description: "内嵌，清晰稳重" },
  { value: "system", label: "系统中文", description: "跟随系统 UI 字体" },
  { value: "wenkai", label: "霞鹜文楷", description: "更轻松，适合阅读" },
  { value: "serif", label: "Noto Serif SC", description: "衬线风格" },
];

export const themeOptions: Array<{ value: ThemeChoice; label: string }> = [
  { value: "light", label: "浅色" },
  { value: "warm", label: "暖灰" },
  { value: "green", label: "护眼绿" },
  { value: "ink", label: "墨色" },
];

export function loadAppearance(): AppearanceSettings {
  if (typeof window === "undefined") {
    return defaultAppearance;
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return defaultAppearance;
    }
    return normalizeAppearance(JSON.parse(raw));
  } catch {
    return defaultAppearance;
  }
}

export function saveAppearance(settings: AppearanceSettings) {
  if (typeof window === "undefined") {
    return;
  }
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

export function applyAppearance(settings: AppearanceSettings) {
  if (typeof document === "undefined") {
    return;
  }
  const root = document.documentElement;
  root.dataset.font = settings.font;
  root.dataset.theme = settings.theme;
}

function normalizeAppearance(value: unknown): AppearanceSettings {
  if (!value || typeof value !== "object") {
    return defaultAppearance;
  }
  const next = value as Partial<AppearanceSettings>;
  return {
    font: isFontChoice(next.font) ? next.font : defaultAppearance.font,
    theme: isThemeChoice(next.theme) ? next.theme : defaultAppearance.theme,
  };
}

function isFontChoice(value: unknown): value is FontChoice {
  return ["noto-sans", "system", "wenkai", "serif"].includes(String(value));
}

function isThemeChoice(value: unknown): value is ThemeChoice {
  return ["light", "warm", "ink", "green"].includes(String(value));
}
