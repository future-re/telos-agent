import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  ExternalLink,
  Globe,
  RotateCw,
} from "lucide-react";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import { Webview } from "@tauri-apps/api/webview";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const DEEPSEEK_URL = "https://chat.deepseek.com";
const DEEPSEEK_PANEL_LABEL = "deepseek-panel";
const DEEPSEEK_WINDOW_LABEL = "deepseek-browser";

type PanelBounds = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export function DeepSeekBrowserPanel() {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [panelError, setPanelError] = useState("");

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let cancelled = false;
    let resizeObserver: ResizeObserver | undefined;

    const sync = async () => {
      const bounds = readBounds(bodyRef.current);
      if (!bounds || cancelled) {
        return;
      }

      try {
        setPanelError("");
        const panel = await ensurePanelWebview(bounds, reloadToken);
        if (cancelled) {
          return;
        }
        await syncPanelWebview(panel, bounds);
      } catch (error) {
        if (!cancelled) {
          setPanelError(`DeepSeek 面板加载失败：${String(error)}`);
        }
      }
    };

    const scheduleSync = () => {
      window.requestAnimationFrame(() => {
        void sync();
      });
    };

    scheduleSync();

    if (bodyRef.current) {
      resizeObserver = new ResizeObserver(scheduleSync);
      resizeObserver.observe(bodyRef.current);
    }
    window.addEventListener("resize", scheduleSync);

    return () => {
      cancelled = true;
      resizeObserver?.disconnect();
      window.removeEventListener("resize", scheduleSync);
      void hidePanelWebview();
    };
  }, [reloadToken]);

  async function openDeepSeekWindow() {
    if (!isTauriRuntime()) {
      window.open(DEEPSEEK_URL, "_blank", "noopener,noreferrer");
      return;
    }

    try {
      const existing = await WebviewWindow.getByLabel(DEEPSEEK_WINDOW_LABEL);
      if (existing) {
        await existing.show();
        await existing.setFocus();
        return;
      }

      const webview = new WebviewWindow(DEEPSEEK_WINDOW_LABEL, {
        url: DEEPSEEK_URL,
        title: "DeepSeek",
        width: 1120,
        height: 760,
        minWidth: 860,
        minHeight: 620,
        focus: true,
      });
      webview.once("tauri://error", (event) => {
        setPanelError(`DeepSeek 窗口打开失败：${String(event.payload ?? "unknown error")}`);
      });
    } catch (error) {
      setPanelError(`DeepSeek 窗口打开失败：${String(error)}`);
    }
  }

  return (
    <div className="grid h-full min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden bg-muted/40">
      <div className="border-b bg-background px-3 py-2.5">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <p className="flex items-center gap-1.5 font-mono text-xs font-bold uppercase text-muted-foreground">
              <Globe className="size-3.5" aria-hidden="true" />
              DeepSeek
            </p>
            <h2 className="mt-1 truncate text-base font-semibold leading-tight tracking-normal">
              官方网页
            </h2>
            <p className="mt-0.5 truncate text-xs text-muted-foreground">{DEEPSEEK_URL}</p>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <BrowserIconButton label="后退" disabled>
              <ArrowLeft className="size-4" aria-hidden="true" />
            </BrowserIconButton>
            <BrowserIconButton label="前进" disabled>
              <ArrowRight className="size-4" aria-hidden="true" />
            </BrowserIconButton>
            <BrowserIconButton label="重新加载" onClick={() => setReloadToken((value) => value + 1)}>
              <RotateCw className="size-4" aria-hidden="true" />
            </BrowserIconButton>
            <BrowserIconButton label="独立窗口打开" onClick={openDeepSeekWindow}>
              <ExternalLink className="size-4" aria-hidden="true" />
            </BrowserIconButton>
          </div>
        </div>
        {panelError ? (
          <p className="mt-2 rounded-md border border-red-200 bg-red-50 px-2 py-1.5 text-xs leading-5 text-red-700">
            {panelError}
          </p>
        ) : null}
      </div>

      <div ref={bodyRef} className="relative min-h-0 min-w-0 overflow-hidden bg-background">
        {!isTauriRuntime() ? (
          <div className="flex h-full items-center justify-center px-6 text-center text-sm text-muted-foreground">
            浏览器预览模式下不会创建原生 WebView。请用 `npm run tauri dev` 启动桌面端。
          </div>
        ) : (
          <div className="flex h-full items-center justify-center px-6 text-center text-sm text-muted-foreground">
            DeepSeek 正在原生 WebView 中加载。
          </div>
        )}
      </div>
    </div>
  );
}

function BrowserIconButton({
  children,
  disabled,
  label,
  onClick,
}: {
  children: React.ReactNode;
  disabled?: boolean;
  label: string;
  onClick?: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="size-8"
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

async function ensurePanelWebview(bounds: PanelBounds, reloadToken: number) {
  const existing = await Webview.getByLabel(DEEPSEEK_PANEL_LABEL);
  if (existing) {
    if (reloadToken > 0) {
      await existing.close().catch(() => undefined);
    } else {
      return existing;
    }
  }

  const webview = new Webview(getCurrentWindow(), DEEPSEEK_PANEL_LABEL, {
    url: DEEPSEEK_URL,
    x: Math.round(bounds.x),
    y: Math.round(bounds.y),
    width: Math.max(1, Math.round(bounds.width)),
    height: Math.max(1, Math.round(bounds.height)),
    focus: true,
  });

  return await waitForWebview(webview);
}

async function syncPanelWebview(webview: Webview, bounds: PanelBounds) {
  await webview.setAutoResize(false);
  await webview.setPosition(new LogicalPosition(bounds.x, bounds.y));
  await webview.setSize(new LogicalSize(bounds.width, bounds.height));
  await webview.show();
}

async function hidePanelWebview() {
  const existing = await Webview.getByLabel(DEEPSEEK_PANEL_LABEL);
  if (existing) {
    await existing.hide().catch(() => undefined);
  }
}

function readBounds(element: HTMLDivElement | null): PanelBounds | null {
  if (!element) {
    return null;
  }

  const rect = element.getBoundingClientRect();
  if (rect.width < 1 || rect.height < 1) {
    return null;
  }

  return {
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
  };
}

function waitForWebview(webview: Webview): Promise<Webview> {
  return new Promise((resolve, reject) => {
    let settled = false;

    const finish = (handler: () => void) => {
      if (settled) {
        return;
      }
      settled = true;
      handler();
    };

    void webview.once("tauri://created", () => {
      finish(() => resolve(webview));
    });
    void webview.once("tauri://error", (event) => {
      finish(() => reject(event.payload ?? "unknown error"));
    });
  });
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
