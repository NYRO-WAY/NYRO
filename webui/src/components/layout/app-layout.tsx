import { useEffect, useState } from "react";
import { Outlet } from "react-router-dom";
import { Github, Languages, LogOut, Moon, Sun } from "lucide-react";

import { Sidebar } from "./sidebar";
import { clearAdminToken, resolveAdminBaseUrl } from "@/lib/admin-auth";
import { IS_TAURI } from "@/lib/backend";
import { useLocale } from "@/lib/i18n";
import { openExternalUrl } from "@/lib/open-external";
import { cn } from "@/lib/utils";

const NON_DRAGGABLE_SELECTOR = [
  "[data-no-drag]",
  "[role]",
  "[tabindex]",
  "[contenteditable='']",
  "[contenteditable='true']",
  "[data-slot]",
  "[cmdk-root]",
  "[cmdk-input]",
  "[cmdk-item]",
  "button",
  "a",
  "input",
  "select",
  "textarea",
  "label",
  "option",
  "summary",
  "svg",
  "path",
].join(",");

export function AppLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const { locale, setLocale } = useLocale();
  const adminBaseUrl = resolveAdminBaseUrl();

  useEffect(() => {
    const saved = localStorage.getItem("nyro-theme");
    const initial =
      saved === "dark" || saved === "light"
        ? saved
        : window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light";
    setTheme(initial);
    document.documentElement.setAttribute("data-theme", initial);
  }, []);

  async function toggleTheme() {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    localStorage.setItem("nyro-theme", next);
    document.documentElement.setAttribute("data-theme", next);
  }

  function toggleLocale() {
    const next = locale === "zh-CN" ? "en-US" : "zh-CN";
    setLocale(next);
  }

  async function openProjectGithub() {
    await openExternalUrl("https://github.com/NYRO-WAY/NYRO");
  }

  function disconnectAdmin() {
    clearAdminToken();
    window.location.reload();
  }

  async function handleSurfaceMouseDown(e: React.MouseEvent<HTMLElement>) {
    if (!IS_TAURI || e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest(NON_DRAGGABLE_SELECTOR)) {
      return;
    }
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startDragging();
    } catch {
      // ignore drag errors on non-desktop context
    }
  }

  const topbarBadge = (
    <div className="hidden max-w-[24rem] truncate rounded-full border border-white/70 bg-white/80 px-3 py-1 text-xs text-slate-500 shadow-sm backdrop-blur xl:block">
      {adminBaseUrl}
    </div>
  );

  const topbarActions = (
    <div className="native-topbar-actions" data-no-drag>
      <button
        onClick={openProjectGithub}
        className="native-action-btn"
        title={locale === "zh-CN" ? "打开 Nyro GitHub" : "Open Nyro on GitHub"}
      >
        <Github className="h-4 w-4" />
      </button>
      <button
        onClick={toggleTheme}
        className="native-action-btn"
        title={theme === "dark" ? "切换浅色模式" : "切换深色模式"}
      >
        {theme === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
      </button>
      <button
        onClick={toggleLocale}
        className="native-action-btn"
        title={locale === "zh-CN" ? "Switch to English" : "切换到中文"}
      >
        <Languages className="h-4 w-4" />
      </button>
      <button
        onClick={disconnectAdmin}
        className="native-action-btn"
        title={locale === "zh-CN" ? "重新登录后台" : "Reconnect admin"}
      >
        <LogOut className="h-4 w-4" />
      </button>
    </div>
  );

  return (
    <div className={cn("app-shell h-screen bg-background", IS_TAURI ? "is-tauri" : "is-web")}>
      {IS_TAURI && (
        <div className="native-topbar">
          <div className="native-topbar-inner">
            <div data-tauri-drag-region className="native-topbar-drag" />
            <div className="absolute left-3 top-3">{topbarBadge}</div>
            {topbarActions}
          </div>
        </div>
      )}

      {!IS_TAURI && (
        <div className="web-topbar">
          <div className="native-topbar-inner">
            <div className="absolute left-3 top-1/2 -translate-y-1/2">{topbarBadge}</div>
            {topbarActions}
          </div>
        </div>
      )}

      <div
        className={cn(
          "layout-frame mx-auto flex h-[calc(100vh-var(--chrome-h))] w-full max-w-[1520px] items-stretch gap-3 overflow-hidden px-3 py-2 md:gap-4 md:px-4 md:py-3",
        )}
      >
        <div onMouseDownCapture={handleSurfaceMouseDown} className="h-full">
          <Sidebar collapsed={collapsed} onToggle={() => setCollapsed(!collapsed)} />
        </div>
        <main
          onMouseDownCapture={handleSurfaceMouseDown}
          className={cn(
            "content-surface h-full min-w-0 flex-1 overflow-y-auto rounded-[1.5rem] border border-white/65 bg-white/56 p-4 shadow-[0_8px_28px_rgba(15,23,42,0.06),inset_0_1px_0_rgba(255,255,255,0.85)] backdrop-blur-xl transition-all duration-300 ease-out md:p-5",
          )}
        >
          <Outlet />
        </main>
      </div>
    </div>
  );
}
