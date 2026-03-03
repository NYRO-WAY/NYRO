import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { AppLayout } from "@/components/layout/app-layout";
import { AppErrorBoundary } from "@/components/error-boundary";
import DashboardPage from "@/pages/dashboard";
import ResourceCrudPage from "@/pages/resource-crud";
import TrafficLogsPage from "@/pages/traffic-logs";
import TrafficStatsPage from "@/pages/traffic-stats";

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
        <BrowserRouter>
          <Routes>
            <Route element={<AppLayout />}>
              <Route index element={<DashboardPage />} />
              <Route path="routes" element={<ResourceCrudPage title="Routes" resource="routes" />} />
              <Route path="services" element={<ResourceCrudPage title="Services" resource="services" />} />
              <Route path="backends" element={<ResourceCrudPage title="Backends" resource="backends" />} />
              <Route path="consumers" element={<ResourceCrudPage title="Consumers" resource="consumers" />} />
              <Route path="plugins" element={<ResourceCrudPage title="Plugins" resource="plugins" />} />
              <Route path="certificates" element={<ResourceCrudPage title="Certificates" resource="certificates" />} />
              <Route path="traffic/logs" element={<TrafficLogsPage />} />
              <Route path="traffic/stats" element={<TrafficStatsPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </QueryClientProvider>
    </AppErrorBoundary>
  </StrictMode>
);
