import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { AppLayout } from "@/components/layout/app-layout";
import { AppErrorBoundary } from "@/components/error-boundary";
import DashboardPage from "@/pages/dashboard";
import ProvidersPage from "@/pages/providers";
import RoutesPage from "@/pages/routes";
import LogsPage from "@/pages/logs";
import StatsPage from "@/pages/stats";
import SettingsPage from "@/pages/settings";
import { LocaleProvider } from "@/lib/i18n";

import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 10_000,
    },
  },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <AppErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <LocaleProvider>
          <BrowserRouter>
            <Routes>
              <Route element={<AppLayout />}>
                <Route index element={<DashboardPage />} />
                <Route path="providers" element={<ProvidersPage />} />
                <Route path="routes" element={<RoutesPage />} />
                <Route path="logs" element={<LogsPage />} />
                <Route path="stats" element={<StatsPage />} />
                <Route path="settings" element={<SettingsPage />} />
              </Route>
            </Routes>
          </BrowserRouter>
        </LocaleProvider>
      </QueryClientProvider>
    </AppErrorBoundary>
  </StrictMode>
);
