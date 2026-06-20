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
  cwdLabel: string;
  projectLabel: string;
  memoryLabel: string;
  apiKeyLabel: string;
  approvalLabel: string;
  activityLabel: string;
  runMetadata: string;
}

export function buildRunDisplay(input: RunDisplayInput): RunDisplay {
  const providerLabel = input.provider === "deepseek" ? "DeepSeek" : "Mock";
  const modelLabel = input.model?.trim() || "auto";
  const cwdLabel = input.cwd?.trim() || "启动目录";
  const projectLabel = input.projectRoot?.trim() || "未检测到项目根目录";
  const memoryLabel = `${input.memoryCount ?? 0} 条记忆`;
  const apiKeyLabel = input.apiKeyConfigured ? "已配置" : "未配置";
  const approvalLabel = input.autoApprove ? "自动批准" : "手动确认";
  const activityLabel = input.running ? "运行中" : "空闲";
  const statusLabel = statusToChinese(input.status);

  return {
    providerLabel,
    modelLabel,
    cwdLabel,
    projectLabel,
    memoryLabel,
    apiKeyLabel,
    approvalLabel,
    activityLabel,
    runMetadata: `${providerLabel} / ${modelLabel} / ${statusLabel}`,
  };
}

function statusToChinese(status: string): string {
  switch (status) {
    case "idle":
      return "空闲";
    case "thinking":
      return "思考中";
    case "streaming":
      return "生成中";
    case "tool completed":
      return "工具完成";
    case "tool failed":
      return "工具失败";
    default:
      return status || "空闲";
  }
}
