import { useState } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "./sidebar";
import { cn } from "@/lib/utils";

export function AppLayout() {
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="min-h-screen bg-background">
      <div className="pointer-events-none fixed inset-0 -z-10 overflow-hidden">
        <div className="absolute -left-28 top-8 h-80 w-80 rounded-full bg-blue-400/25 blur-3xl" />
        <div className="absolute right-0 top-0 h-96 w-96 rounded-full bg-indigo-400/25 blur-3xl" />
        <div className="absolute bottom-[-10rem] left-1/3 h-96 w-96 rounded-full bg-cyan-300/20 blur-3xl" />
      </div>
      <div className="mx-auto flex w-full max-w-[1520px] items-start gap-4 px-4 py-4 md:gap-5 md:px-6">
        <Sidebar collapsed={collapsed} onToggle={() => setCollapsed(!collapsed)} />
        <main
          className={cn(
            "min-h-screen min-w-0 flex-1 transition-all duration-300 ease-out"
          )}
        >
          <Outlet />
        </main>
      </div>
    </div>
  );
}
