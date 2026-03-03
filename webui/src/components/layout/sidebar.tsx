import { NavLink } from "react-router-dom";
import { cn } from "@/lib/utils";
import {
  LayoutDashboard,
  Route,
  Server,
  HardDrive,
  Users,
  Puzzle,
  ShieldCheck,
  ScrollText,
  BarChart3,
  ChevronLeft,
  Bot,
  Sparkles,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

const NAV_ITEMS = [
  { label: "Dashboard", path: "/", icon: LayoutDashboard },
  { label: "Routes", path: "/routes", icon: Route },
  { label: "Services", path: "/services", icon: Server },
  { label: "Backends", path: "/backends", icon: HardDrive },
  { label: "Consumers", path: "/consumers", icon: Users },
  { label: "Plugins", path: "/plugins", icon: Puzzle },
  { label: "Certificates", path: "/certificates", icon: ShieldCheck },
  { type: "divider" as const },
  { label: "Traffic Logs", path: "/traffic/logs", icon: ScrollText },
  { label: "Traffic Stats", path: "/traffic/stats", icon: BarChart3 },
] as const;

interface SidebarProps {
  collapsed: boolean;
  onToggle: () => void;
}

export function Sidebar({ collapsed, onToggle }: SidebarProps) {
  return (
    <aside
      className={cn(
        "glass-strong sticky top-4 z-30 flex h-[calc(100vh-2rem)] shrink-0 flex-col rounded-[1.6rem] transition-all duration-300 ease-out",
        collapsed ? "w-[4.5rem]" : "w-[16rem]"
      )}
    >
      {/* Logo */}
      <div className="flex h-16 items-center gap-3 px-4">
        <img
          src="/assets/logos/NYRO-logo.png"
          alt="Nyro"
          className="h-8 w-8 shrink-0 rounded-md object-contain"
        />
        {!collapsed && (
          <span className="text-[17px] font-semibold tracking-tight text-slate-900">
            Nyro
          </span>
        )}
      </div>

      {/* Navigation */}
      <nav className="flex-1 space-y-1 overflow-y-auto px-3 py-3">
        {NAV_ITEMS.map((item, i) => {
          if ("type" in item && item.type === "divider") {
            return (
              <div key={i} className="my-3 border-t border-slate-200/80" />
            );
          }
          const { label, path, icon: Icon } = item as {
            label: string;
            path: string;
            icon: LucideIcon;
          };
          return (
            <NavLink
              key={path}
              to={path}
              end={path === "/"}
              className={({ isActive }) =>
                cn(
                  "group flex items-center gap-3 rounded-xl px-3 py-2.5 text-[13px] font-medium transition-all duration-200 cursor-pointer",
                  isActive
                    ? "bg-slate-900 text-white shadow-md"
                    : "text-slate-600 hover:bg-white hover:text-slate-900"
                )
              }
            >
              <Icon className="h-4 w-4 shrink-0" />
              {!collapsed && <span>{label}</span>}
            </NavLink>
          );
        })}
      </nav>

      {!collapsed && (
        <div className="px-3 pb-3">
          <div className="rounded-2xl border border-white/80 bg-white/65 px-3 py-2.5 text-slate-700 shadow-[inset_0_1px_0_rgba(255,255,255,0.9)]">
            <div className="flex items-center gap-2 text-[12px] font-medium">
              <Sparkles className="h-3.5 w-3.5 text-blue-600" />
              AI Gateway Mode
            </div>
            <div className="mt-1 flex items-center gap-2 text-[11px] text-slate-500">
              <Bot className="h-3.5 w-3.5" />
              Hybrid Observability Enabled
            </div>
          </div>
        </div>
      )}

      {/* Collapse Toggle */}
      <button
        onClick={onToggle}
        className="flex h-11 items-center justify-center border-t border-slate-200/80 text-slate-500 transition-colors hover:text-slate-900 cursor-pointer"
      >
        <ChevronLeft
          className={cn(
            "h-4 w-4 transition-transform duration-300",
            collapsed && "rotate-180"
          )}
        />
      </button>
    </aside>
  );
}
