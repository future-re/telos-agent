export type ProviderKind = "mock" | "deepseek";

export interface RunDisplayInput {
  provider: ProviderKind;
  model?: string;
  cwd?: string;
  projectRoot?: string;
  memoryCount?: number;
  apiKeyConfigured?: boolean;
  autoApprove: boolean;
  status: string;
  running: boolean;
}

export interface RunDisplay {
  providerLabel: string;
  modelLabel: string;
  modelDescription: string;
  cwdLabel: string;
  projectLabel: string;
  workspaceLabel: string;
  memoryLabel: string;
  apiKeyLabel: string;
  approvalLabel: string;
  activityLabel: string;
  runMetadata: string;
}

export function buildRunDisplay(input: RunDisplayInput): RunDisplay {
  const providerLabel = input.provider === "deepseek" ? "DeepSeek" : "Mock";
  const model = normalizeModel(input.model);
  const modelLabel = model.label;
  const cwdLabel = input.cwd?.trim() || "启动目录";
  const projectLabel = input.projectRoot?.trim() || "未检测到项目根目录";
  const workspaceLabel = input.projectRoot?.trim() || cwdLabel;
  const memoryLabel = `${input.memoryCount ?? 0} 条记忆`;
  const apiKeyLabel = input.apiKeyConfigured ? "已配置" : "未配置";
  const approvalLabel = input.autoApprove ? "自动批准" : "手动确认";
  const activityLabel = input.running ? "运行中" : "空闲";

  return {
    providerLabel,
    modelLabel,
    modelDescription: model.description,
    cwdLabel,
    projectLabel,
    workspaceLabel,
    memoryLabel,
    apiKeyLabel,
    approvalLabel,
    activityLabel,
    runMetadata: `${providerLabel} / ${modelLabel}`,
  };
}

function normalizeModel(model?: string): {
  label: string;
  description: string;
} {
  const value = model?.trim().toLowerCase();
  switch (value) {
    case "":
    case undefined:
    case "flash":
    case "deepseek-v4-flash":
      return {
        label: "DeepSeek V4 Flash",
        description: "适合快速响应和轻量任务",
      };
    case "pro":
    case "deepseek-v4-pro":
      return { label: "DeepSeek V4 Pro", description: "适合复杂推理和规划" };
    default:
      return {
        label: "DeepSeek V4 Flash",
        description: "适合快速响应和轻量任务",
      };
  }
}
