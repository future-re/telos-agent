import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface DesktopPluginInfo {
  id: string;
  name: string;
  description?: string;
  version: string;
  sourceStatus: "available" | "removed-from-marketplace" | "marketplace-missing";
  status: "enabled" | "disabled" | "degraded" | "error";
  errors: string[];
  configSchema: Record<string, unknown> | null;
  config: Record<string, unknown> | null;
}

interface DesktopMarketplacePlugin {
  id: string;
  name: string;
  description?: string;
  version: string;
  installed: boolean;
}

export function PluginSettings({ cwd }: { cwd: string }) {
  const [plugins, setPlugins] = useState<DesktopPluginInfo[]>([]);
  const [marketplacePlugins, setMarketplacePlugins] = useState<DesktopMarketplacePlugin[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();

  const refresh = useCallback(async () => {
    try {
      setError(undefined);
      const request = { cwd: cwd || undefined };
      const [installed, available] = await Promise.all([
        invoke<DesktopPluginInfo[]>("list_plugins", { request }),
        invoke<DesktopMarketplacePlugin[]>("list_marketplace_plugins", { request }),
      ]);
      setPlugins(installed);
      setMarketplacePlugins(available);
    } catch (reason) {
      setError(String(reason));
    }
  }, [cwd]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    setDrafts(
      Object.fromEntries(
        plugins.map((plugin) => [
          plugin.id,
          JSON.stringify(editableConfig(plugin), null, 2),
        ]),
      ),
    );
  }, [plugins]);

  async function mutate(id: string, command: string, extra = {}) {
    setBusy(id);
    setError(undefined);
    try {
      await invoke(command, {
        request: { cwd: cwd || undefined, id, ...extra },
      });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(undefined);
    }
  }

  async function saveConfig(id: string) {
    try {
      const values = JSON.parse(drafts[id] || "{}") as Record<string, unknown>;
      if (!values || Array.isArray(values) || typeof values !== "object") {
        throw new Error("配置必须是 JSON 对象");
      }
      await mutate(id, "set_plugin_config", { values });
    } catch (reason) {
      setError(String(reason));
    }
  }

  return (
    <div className="grid gap-3">
      <div className="flex items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">
          启停和配置会持久化；已运行的会话需重新创建后生效。
        </p>
        <Button type="button" size="sm" variant="outline" onClick={() => void refresh()}>
          <RefreshCw className="size-3.5" aria-hidden="true" />
          刷新
        </Button>
      </div>
      {error && (
        <div role="alert" className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
          {error}
        </div>
      )}
      {plugins.length === 0 ? (
        <div className="rounded-md border border-dashed px-3 py-5 text-center text-sm text-muted-foreground">
          当前项目没有已安装插件。可从下方已注册的 marketplace 安装，或使用 CLI 添加来源。
        </div>
      ) : (
        <div className="grid max-h-[430px] gap-2 overflow-y-auto pr-1">
          {plugins.map((plugin) => {
            const active = plugin.status === "enabled" || plugin.status === "degraded";
            const replaceable = plugin.status === "disabled";
            return (
              <article key={plugin.id} className="rounded-md border bg-background p-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <h4 className="truncate text-sm font-semibold">{plugin.name}</h4>
                    <p className="truncate font-mono text-[11px] text-muted-foreground">
                      {plugin.id} · {plugin.version}
                    </p>
                    {plugin.description && <p className="mt-1 text-xs text-muted-foreground">{plugin.description}</p>}
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant={active ? "outline" : "default"}
                    disabled={busy === plugin.id}
                    onClick={() => void mutate(plugin.id, "set_plugin_enabled", { enabled: !active })}
                  >
                    {active ? "停用" : "启用"}
                  </Button>
                </div>
                <div className="mt-2 text-xs">
                  状态：<span className="font-medium">{plugin.status}</span>
                </div>
                {plugin.sourceStatus !== "available" && (
                  <p className="mt-1 text-xs text-amber-600">
                    来源：{plugin.sourceStatus === "removed-from-marketplace" ? "已从 marketplace 删除" : "marketplace 已移除"}
                  </p>
                )}
                <div className="mt-2 flex gap-2">
                  <Button type="button" size="sm" variant="outline" disabled={busy === plugin.id || !replaceable} title={!replaceable ? "请先停用插件" : undefined} onClick={() => void mutate(plugin.id, "upgrade_plugin")}>升级</Button>
                  <Button type="button" size="sm" variant="outline" disabled={busy === plugin.id || active} title={active ? "请先停用插件" : undefined} onClick={() => void mutate(plugin.id, "uninstall_plugin")}>卸载</Button>
                </div>
                {plugin.errors.map((message) => (
                  <p key={message} className="mt-1 text-xs text-destructive">{message}</p>
                ))}
                {plugin.configSchema && Object.keys(plugin.configSchema).length > 0 && (
                  <div className="mt-3 grid gap-2 border-t pt-3">
                    <details className="text-xs text-muted-foreground">
                      <summary className="cursor-pointer">配置字段与当前脱敏值</summary>
                      <pre className="mt-2 overflow-x-auto whitespace-pre-wrap rounded bg-muted p-2 text-[11px]">
                        {JSON.stringify({ schema: plugin.configSchema, current: plugin.config }, null, 2)}
                      </pre>
                    </details>
                    <Textarea
                      value={drafts[plugin.id] ?? "{}"}
                      onChange={(event) => setDrafts((current) => ({ ...current, [plugin.id]: event.target.value }))}
                      aria-label={`${plugin.name} JSON 配置`}
                      className="min-h-20 font-mono text-xs"
                    />
                    <div className="flex gap-2">
                      <Button type="button" size="sm" disabled={busy === plugin.id} onClick={() => void saveConfig(plugin.id)}>
                        保存配置
                      </Button>
                      <Button type="button" size="sm" variant="outline" disabled={busy === plugin.id} onClick={() => void mutate(plugin.id, "clear_plugin_config")}>
                        清空配置
                      </Button>
                    </div>
                  </div>
                )}
              </article>
            );
          })}
        </div>
      )}
      {marketplacePlugins.some((plugin) => !plugin.installed) && (
        <section className="grid gap-2 border-t pt-3">
          <div>
            <h4 className="text-sm font-semibold">可安装插件</h4>
            <p className="text-xs text-muted-foreground">来自已注册的 marketplace。</p>
          </div>
          {marketplacePlugins.filter((plugin) => !plugin.installed).map((plugin) => (
            <article key={plugin.id} className="flex items-start justify-between gap-3 rounded-md border bg-background p-3">
              <div className="min-w-0">
                <h5 className="truncate text-sm font-medium">{plugin.name}</h5>
                <p className="truncate font-mono text-[11px] text-muted-foreground">{plugin.id} · {plugin.version}</p>
                {plugin.description && <p className="mt-1 text-xs text-muted-foreground">{plugin.description}</p>}
              </div>
              <Button type="button" size="sm" disabled={busy === plugin.id} onClick={() => void mutate(plugin.id, "install_plugin")}>安装</Button>
            </article>
          ))}
        </section>
      )}
    </div>
  );
}

export function editableConfig(plugin: DesktopPluginInfo): Record<string, unknown> {
  const current = plugin.config ?? {};
  const schema = plugin.configSchema ?? {};
  return Object.fromEntries(
    Object.entries(current).filter(([key, value]) => {
      const option = schema[key];
      const sensitive =
        option !== null &&
        typeof option === "object" &&
        "sensitive" in option &&
        option.sensitive === true;
      return !sensitive && value !== "[REDACTED]";
    }),
  );
}
