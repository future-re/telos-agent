import { ProviderKind } from "@/runDisplay";

export interface DesktopSettingsOverrides {
  provider?: ProviderKind;
  apiKey?: string;
  model?: string;
  cwd?: string;
  maxIterations?: number;
  autoApprove?: boolean;
}

export interface ResolvedDesktopSettings {
  provider: ProviderKind;
  model: string;
  cwd: string;
  projectRoot?: string;
  projectRootOrCwd: string;
  memoryRoot: string;
  memoryCount: number;
  apiKeyConfigured: boolean;
  autoApprove: boolean;
  maxIterations: number;
  configPath?: string;
  instructionsFile?: string;
}
