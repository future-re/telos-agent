import { describe, expect, it } from "vitest";
import { buildRunDisplay } from "./runDisplay";

describe("buildRunDisplay", () => {
  it("formats Chinese compact labels for the desktop run inspector", () => {
    expect(
      buildRunDisplay({
        provider: "deepseek",
        model: "deepseek-v4-pro",
        cwd: "C:\\work\\telos",
        projectRoot: "C:\\work\\telos",
        memoryCount: 3,
        apiKeyConfigured: true,
        autoApprove: true,
        status: "thinking",
        running: true,
      }),
    ).toEqual({
      providerLabel: "DeepSeek",
      modelLabel: "DeepSeek V4 Pro",
      modelDescription: "适合复杂推理和规划",
      cwdLabel: "C:\\work\\telos",
      projectLabel: "C:\\work\\telos",
      workspaceLabel: "C:\\work\\telos",
      memoryLabel: "3 条记忆",
      apiKeyLabel: "已配置",
      approvalLabel: "自动批准",
      activityLabel: "运行中",
      runMetadata: "DeepSeek / DeepSeek V4 Pro",
    });
  });

  it("uses readable defaults for empty optional settings", () => {
    expect(
      buildRunDisplay({
        provider: "mock",
        model: "",
        cwd: "",
        memoryCount: 0,
        apiKeyConfigured: false,
        autoApprove: false,
        status: "idle",
        running: false,
      }),
    ).toMatchObject({
      providerLabel: "Mock",
      modelLabel: "自动路由",
      modelDescription: "按任务自动选择 Pro 或 Flash",
      cwdLabel: "启动目录",
      projectLabel: "未检测到项目根目录",
      workspaceLabel: "启动目录",
      memoryLabel: "0 条记忆",
      apiKeyLabel: "未配置",
      approvalLabel: "手动确认",
      activityLabel: "空闲",
    });
  });
});
