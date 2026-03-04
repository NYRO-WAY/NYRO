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

  return (
    <div className={cn("app-shell min-h-screen bg-background", IS_TAURI ? "is-tauri" : "is-web")}>
      {IS_TAURI && (
        <div className="native-topbar">
          <div data-tauri-drag-region className="native-topbar-drag" />
          <div className="native-topbar-actions">
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
      )}

      {!IS_TAURI && (
        <div className="web-topbar">
          <div className="native-topbar-actions">
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
      )}

      <div
        className={cn(
          "layout-frame mx-auto flex w-full max-w-[1520px] items-start gap-3 px-3 pb-3 md:gap-4 md:px-4 md:pb-4",
          IS_TAURI ? "pt-2" : "pt-3"
        )}
      >
        <Sidebar collapsed={collapsed} onToggle={() => setCollapsed(!collapsed)} />
        <main
          className={cn(
            "content-surface min-h-[calc(100vh-var(--chrome-h)-1rem)] min-w-0 flex-1 rounded-[1.5rem] border border-white/65 bg-white/56 p-4 shadow-[0_8px_28px_rgba(15,23,42,0.06),inset_0_1px_0_rgba(255,255,255,0.85)] backdrop-blur-xl transition-all duration-300 ease-out md:p-5"
          )}
        >
          <Outlet />
        </main>
      </div>
    </div>
  );
}
