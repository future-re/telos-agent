import { ProviderKind } from "@/runDisplay";

export type SettingsSection =
  | "appearance"
  | "service"
  | "key"
  | "approval"
  | "model"
  | "directory";

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

export interface MemoryBucket {
  label: string;
  count: number;
}

export interface MemoryPreview {
  name: string;
  description: string;
  category: string;
  status: string;
  updated: string;
  timesUsed: number;
  tags: string[];
}

export interface MemoryOverview {
  root: string;
  total: number;
  categories: MemoryBucket[];
  statuses: MemoryBucket[];
  recent: MemoryPreview[];
}
