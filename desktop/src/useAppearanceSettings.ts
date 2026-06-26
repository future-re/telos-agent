import { useEffect, useState } from "react";
import {
  AppearanceSettings,
  applyAppearance,
  loadAppearance,
  saveAppearance,
} from "@/appearance";

export function useAppearanceSettings() {
  const [appearance, setAppearance] = useState<AppearanceSettings>(() =>
    loadAppearance(),
  );

  useEffect(() => {
    applyAppearance(appearance);
    saveAppearance(appearance);
  }, [appearance]);

  return { appearance, setAppearance };
}
