import { useEffect, useState } from "react";
import { Outlet } from "react-router-dom";
import { Languages, Moon, Sun } from "lucide-react";
import { Sidebar } from "./sidebar";
import { cn } from "@/lib/utils";
import { IS_TAURI } from "@/lib/backend";
import { useLocale } from "@/lib/i18n";

export function AppLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const { locale, setLocale } = useLocale();

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

  async function handleSurfaceMouseDown(e: React.MouseEvent<HTMLElement>) {
    if (!IS_TAURI || e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (
      target.closest(
        "button,a,input,select,textarea,label,[role='button'],[data-no-drag],svg,path"
      )
    ) {
      return;
    }
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startDragging();
    } catch {
      // ignore drag errors on non-desktop context
    }
  }

  return (
    <div className={cn("app-shell h-screen bg-background", IS_TAURI ? "is-tauri" : "is-web")}>
      {IS_TAURI && (
        <div className="native-topbar">
          <div className="native-topbar-inner">
            <div data-tauri-drag-region className="native-topbar-drag" />
            <div className="native-topbar-actions" data-no-drag>
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
            </div>
          </div>
        </div>
      )}

      {!IS_TAURI && (
        <div className="web-topbar">
          <div className="native-topbar-inner">
            <div className="native-topbar-actions" data-no-drag>
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
            </div>
          </div>
        </div>
      )}

      <div
        className={cn(
          "layout-frame mx-auto flex h-[calc(100vh-var(--chrome-h))] w-full max-w-[1520px] items-stretch gap-3 overflow-hidden px-3 py-2 md:gap-4 md:px-4 md:py-3"
        )}
      >
        <div onMouseDownCapture={handleSurfaceMouseDown} className="h-full">
          <Sidebar collapsed={collapsed} onToggle={() => setCollapsed(!collapsed)} />
        </div>
        <main
          onMouseDownCapture={handleSurfaceMouseDown}
          className={cn(
            "content-surface h-full min-w-0 flex-1 overflow-y-auto rounded-[1.5rem] border border-white/65 bg-white/56 p-4 shadow-[0_8px_28px_rgba(15,23,42,0.06),inset_0_1px_0_rgba(255,255,255,0.85)] backdrop-blur-xl transition-all duration-300 ease-out md:p-5"
          )}
        >
          <Outlet />
        </main>
      </div>
    </div>
  );
}
