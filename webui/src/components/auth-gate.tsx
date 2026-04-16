import { useEffect, useState } from "react";
import { Outlet } from "react-router-dom";
import { AlertCircle, ArrowRight, CheckCircle2 } from "lucide-react";

import NyroLogo from "@/assets/logos/NYRO-logo.png";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  AdminAuthError,
  clearAdminToken,
  getAdminConnectionConfig,
  probeAdminConnection,
  saveAdminConnectionConfig,
} from "@/lib/admin-auth";

function LoginScreen({
  initialToken,
  error,
  onSubmit,
}: {
  initialToken: string;
  error: string;
  onSubmit: (payload: { token: string }) => Promise<void>;
}) {
  const [token, setToken] = useState(initialToken);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => setToken(initialToken), [initialToken]);

  async function handleSubmit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setSubmitting(true);
    try {
      await onSubmit({ token });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top,_rgba(255,255,255,0.96),_transparent_28%),linear-gradient(180deg,_#f3f4f6_0%,_#e5e7eb_100%)] px-4 py-8 text-slate-900">
      <div className="mx-auto flex min-h-[calc(100vh-4rem)] max-w-md items-center justify-center">
        <section className="w-full rounded-[2rem] border border-white/70 bg-white/92 p-7 shadow-[0_24px_80px_rgba(15,23,42,0.14)] backdrop-blur-xl sm:p-8">
          <div className="flex flex-col items-center text-center">
            <div className="flex h-16 w-16 items-center justify-center rounded-[1.35rem] bg-white shadow-[0_10px_30px_rgba(15,23,42,0.10)] ring-1 ring-slate-200/70">
              <img src={NyroLogo} alt="NYRO" className="h-11 w-11 object-contain" />
            </div>
            <h1 className="mt-5 text-2xl font-semibold tracking-tight text-slate-950">登录</h1>
          </div>

          <form className="mt-8 space-y-5" onSubmit={handleSubmit}>
            <div>
              <label className="mb-2 block text-sm font-medium text-slate-700">Admin Token</label>
              <Input type="password" value={token} onChange={(e) => setToken(e.target.value)} placeholder="" />
            </div>

            {error && (
              <div className="flex items-start gap-2 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
                <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                <span>{error}</span>
              </div>
            )}

            <Button type="submit" size="lg" className="h-11 w-full rounded-xl" disabled={submitting}>
              {submitting ? "登录中..." : "登录"}
              {!submitting && <ArrowRight className="h-4 w-4" />}
            </Button>
          </form>
        </section>
      </div>
    </div>
  );
}

export function AuthGate() {
  const [ready, setReady] = useState(false);
  const [authenticated, setAuthenticated] = useState(false);
  const [error, setError] = useState("");
  const [snapshot, setSnapshot] = useState(() => getAdminConnectionConfig());

  useEffect(() => {
    const sync = () => setSnapshot(getAdminConnectionConfig());
    window.addEventListener("nyro-admin-connection-change", sync);
    return () => window.removeEventListener("nyro-admin-connection-change", sync);
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function bootstrap() {
      try {
        const config = getAdminConnectionConfig();
        await probeAdminConnection(config.baseUrl, config.token);
        if (!cancelled) {
          setAuthenticated(true);
          setError("");
        }
      } catch (err) {
        if (!cancelled) {
          if (err instanceof AdminAuthError && (err.status === 401 || err.status === 403)) {
            setAuthenticated(false);
            setError("");
          } else if (err instanceof Error) {
            setAuthenticated(false);
            setError(`连接失败：${err.message}`);
          } else {
            setAuthenticated(false);
            setError("连接失败");
          }
        }
      } finally {
        if (!cancelled) setReady(true);
      }
    }

    void bootstrap();
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleLogin(payload: { token: string }) {
    try {
      const result = saveAdminConnectionConfig({ token: payload.token });
      await probeAdminConnection(result.baseUrl, result.token);
      setSnapshot(result);
      setAuthenticated(true);
      setError("");
    } catch (err) {
      setAuthenticated(false);
      if (err instanceof AdminAuthError && (err.status === 401 || err.status === 403)) {
        clearAdminToken();
        setError("Token 不正确");
        return;
      }
      setError(err instanceof Error ? `连接失败：${err.message}` : "连接失败");
    }
  }

  if (!ready) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-[linear-gradient(180deg,_#f3f4f6_0%,_#e5e7eb_100%)]">
        <div className="flex items-center gap-3 rounded-full border border-white/70 bg-white/80 px-5 py-3 text-sm text-slate-600 shadow-lg backdrop-blur-xl">
          <CheckCircle2 className="h-4 w-4 animate-pulse text-emerald-600" />
          正在检查后台连接...
        </div>
      </div>
    );
  }

  if (!authenticated) {
    return <LoginScreen initialToken={snapshot.token} error={error} onSubmit={handleLogin} />;
  }

  return <Outlet />;
}
