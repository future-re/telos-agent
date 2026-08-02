// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PluginSettings, editableConfig } from "@/components/PluginSettings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("editableConfig", () => {
  it("keeps ordinary values in the editor while excluding redacted secrets", () => {
    expect(
      editableConfig({
        id: "configured@test",
        name: "configured",
        version: "1.0.0",
        status: "enabled",
        sourceStatus: "available",
        errors: [],
        configSchema: {
          token: { type: "string", sensitive: true },
          mode: { type: "string" },
        },
        config: { token: "[REDACTED]", mode: "strict" },
      }),
    ).toEqual({ mode: "strict" });
  });

  it("drives enable, configuration, refresh, and error interactions through Tauri", async () => {
    const invokeMock = vi.mocked(invoke);
    invokeMock.mockImplementation(async (command) => {
      if (command === "list_plugins") {
        return [{
          id: "configured@test",
          name: "configured",
          version: "1.0.0",
          sourceStatus: "available",
          status: "disabled",
          errors: [],
          configSchema: {
            token: { type: "string", sensitive: true },
            mode: { type: "string" },
          },
          config: { token: "[REDACTED]", mode: "safe" },
        }];
      }
      if (command === "list_marketplace_plugins") {
        return [{
          id: "configured@test",
          name: "configured",
          version: "1.0.0",
          installed: true,
        }];
      }
      if (command === "upgrade_plugin") {
        throw new Error("version conflict");
      }
      return undefined;
    });
    const user = userEvent.setup();
    render(createElement(PluginSettings, { cwd: "/project" }));

    expect(await screen.findByText("configured")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "启用" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_plugin_enabled", {
        request: { cwd: "/project", id: "configured@test", enabled: true },
      });
    });

    const editor = screen.getByLabelText("configured JSON 配置");
    fireEvent.change(editor, { target: { value: '{"mode":"strict"}' } });
    await user.click(screen.getByRole("button", { name: "保存配置" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_plugin_config", {
        request: {
          cwd: "/project",
          id: "configured@test",
          values: { mode: "strict" },
        },
      });
    });

    await user.click(screen.getByRole("button", { name: "升级" }));
    expect((await screen.findByRole("alert")).textContent).toContain("version conflict");
    await user.click(screen.getByRole("button", { name: "刷新" }));
    await waitFor(() => {
      expect(invokeMock.mock.calls.filter(([command]) => command === "list_plugins").length)
        .toBeGreaterThan(3);
    });
  });

  it("installs an available marketplace plugin", async () => {
    const invokeMock = vi.mocked(invoke);
    invokeMock.mockImplementation(async (command) => {
      if (command === "list_plugins") return [];
      if (command === "list_marketplace_plugins") {
        return [{
          id: "formatter@test",
          name: "formatter",
          version: "1.0.0",
          installed: false,
        }];
      }
      return undefined;
    });
    const user = userEvent.setup();
    render(createElement(PluginSettings, { cwd: "/project" }));

    await user.click(await screen.findByRole("button", { name: "安装" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("install_plugin", {
        request: { cwd: "/project", id: "formatter@test" },
      });
    });
  });

  it("requires an active plugin to be disabled before upgrade", async () => {
    const invokeMock = vi.mocked(invoke);
    invokeMock.mockImplementation(async (command) => {
      if (command === "list_plugins") {
        return [{
          id: "active@test",
          name: "active",
          version: "1.0.0",
          sourceStatus: "available",
          status: "enabled",
          errors: [],
          configSchema: null,
          config: null,
        }];
      }
      if (command === "list_marketplace_plugins") return [];
      return undefined;
    });
    render(createElement(PluginSettings, { cwd: "/project" }));

    const upgrade = await screen.findByRole("button", { name: "升级" });
    expect((upgrade as HTMLButtonElement).disabled).toBe(true);
    expect(upgrade.getAttribute("title")).toBe("请先停用插件");
  });

  it("serializes all plugin mutations and refreshes in the UI", async () => {
    const invokeMock = vi.mocked(invoke);
    let releaseMutation: (() => void) | undefined;
    const pendingMutation = new Promise<void>((resolve) => {
      releaseMutation = resolve;
    });
    invokeMock.mockImplementation(async (command) => {
      if (command === "list_plugins") {
        return ["one", "two"].map((name) => ({
          id: `${name}@test`,
          name,
          version: "1.0.0",
          sourceStatus: "available",
          status: "disabled",
          errors: [],
          configSchema: null,
          config: null,
        }));
      }
      if (command === "list_marketplace_plugins") return [];
      if (command === "set_plugin_enabled") return pendingMutation;
      return undefined;
    });
    const user = userEvent.setup();
    render(createElement(PluginSettings, { cwd: "/project" }));

    const enableButtons = await screen.findAllByRole("button", { name: "启用" });
    await user.click(enableButtons[0]);

    await waitFor(() => {
      expect(enableButtons.every((button) => (button as HTMLButtonElement).disabled)).toBe(true);
      expect((screen.getByRole("button", { name: "刷新" }) as HTMLButtonElement).disabled)
        .toBe(true);
    });

    releaseMutation?.();
    await waitFor(() => {
      expect((screen.getByRole("button", { name: "刷新" }) as HTMLButtonElement).disabled)
        .toBe(false);
    });
  });
});
