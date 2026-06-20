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
      modelLabel: "deepseek-v4-pro",
      cwdLabel: "C:\\work\\telos",
      projectLabel: "C:\\work\\telos",
      memoryLabel: "3 条记忆",
      apiKeyLabel: "已配置",
      approvalLabel: "自动批准",
      activityLabel: "运行中",
      runMetadata: "DeepSeek / deepseek-v4-pro / 思考中",
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
      modelLabel: "auto",
      cwdLabel: "启动目录",
      projectLabel: "未检测到项目根目录",
      memoryLabel: "0 条记忆",
      apiKeyLabel: "未配置",
      approvalLabel: "手动确认",
      activityLabel: "空闲",
    });
  });
});
