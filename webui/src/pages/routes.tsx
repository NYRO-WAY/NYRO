import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { backend } from "@/lib/backend";
import type { Route as RouteType, CreateRoute, Provider } from "@/lib/types";
import { Route as RouteIcon, Plus, Trash2, Pencil, X } from "lucide-react";

interface UpdateRoutePayload {
  name?: string;
  match_pattern?: string;
  target_provider?: string;
  target_model?: string;
  fallback_provider?: string;
  fallback_model?: string;
  is_active?: boolean;
  priority?: number;
}

export default function RoutesPage() {
  const qc = useQueryClient();
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  const { data: routes = [], isLoading } = useQuery<RouteType[]>({
    queryKey: ["routes"],
    queryFn: () => backend("list_routes"),
  });

  const { data: providers = [] } = useQuery<Provider[]>({
    queryKey: ["providers"],
    queryFn: () => backend("get_providers"),
  });

  const createMut = useMutation({
    mutationFn: (input: CreateRoute) => backend("create_route", { input }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["routes"] });
      setShowForm(false);
      setForm(emptyCreate);
    },
  });

  const [editError, setEditError] = useState<string | null>(null);

  const updateMut = useMutation({
    mutationFn: ({ id, ...input }: UpdateRoutePayload & { id: string }) =>
      backend("update_route", { id, input }),
    onSuccess: () => {
      setEditError(null);
      qc.invalidateQueries({ queryKey: ["routes"] });
      setEditingId(null);
    },
    onError: (err: Error) => {
      setEditError(String(err));
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => backend("delete_route", { id }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["routes"] }),
  });

  const emptyCreate: CreateRoute = {
    name: "",
    match_pattern: "*",
    target_provider: "",
    target_model: "",
  };

  const [form, setForm] = useState<CreateRoute>(emptyCreate);

  const [editForm, setEditForm] = useState<UpdateRoutePayload & { id: string }>({
    id: "",
    name: "",
    match_pattern: "",
    target_provider: "",
    target_model: "",
    fallback_provider: "",
    fallback_model: "",
    is_active: true,
    priority: 0,
  });

  function startEdit(r: RouteType) {
    setEditingId(r.id);
    setEditForm({
      id: r.id,
      name: r.name,
      match_pattern: r.match_pattern,
      target_provider: r.target_provider,
      target_model: r.target_model,
      fallback_provider: r.fallback_provider ?? "",
      fallback_model: r.fallback_model ?? "",
      is_active: r.is_active,
      priority: r.priority,
    });
  }

  function providerName(id: string) {
    return providers.find((p) => p.id === id)?.name ?? id.slice(0, 8);
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">Routes</h1>
          <p className="mt-1 text-sm text-slate-500">Model-based routing rules</p>
        </div>
        <button
          onClick={() => { setShowForm(!showForm); setEditingId(null); }}
          className="flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2.5 text-sm font-medium text-white shadow-md transition-all hover:bg-slate-800 cursor-pointer"
        >
          <Plus className="h-4 w-4" />
          Add Route
        </button>
      </div>

      {/* Create Form */}
      {showForm && (
        <div className="glass rounded-2xl p-6 space-y-4">
          <h2 className="text-lg font-semibold text-slate-900">New Route</h2>
          <div className="grid grid-cols-2 gap-4">
            <input
              placeholder="Name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
            <input
              placeholder="Match Pattern (e.g. gpt-4*, claude-*, *)"
              value={form.match_pattern}
              onChange={(e) => setForm({ ...form, match_pattern: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
            <select
              value={form.target_provider}
              onChange={(e) => setForm({ ...form, target_provider: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            >
              <option value="">Select Provider</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </select>
            <input
              placeholder="Target Model (e.g. gpt-4o, or * for passthrough)"
              value={form.target_model}
              onChange={(e) => setForm({ ...form, target_model: e.target.value })}
              className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
            />
          </div>
          <div className="flex gap-3">
            <button
              onClick={() => createMut.mutate(form)}
              disabled={createMut.isPending || !form.name || !form.target_provider}
              className="rounded-xl bg-slate-900 px-5 py-2 text-sm font-medium text-white hover:bg-slate-800 cursor-pointer disabled:opacity-50"
            >
              {createMut.isPending ? "Creating..." : "Create"}
            </button>
            <button
              onClick={() => { setShowForm(false); setForm(emptyCreate); }}
              className="rounded-xl border border-slate-200 px-5 py-2 text-sm font-medium text-slate-600 hover:bg-slate-50 cursor-pointer"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* List */}
      {isLoading ? (
        <div className="text-center text-sm text-slate-500 py-12">Loading...</div>
      ) : routes.length === 0 ? (
        <div className="glass rounded-2xl p-12 text-center">
          <RouteIcon className="mx-auto h-10 w-10 text-slate-400" />
          <p className="mt-3 text-sm text-slate-500">No routes configured</p>
        </div>
      ) : (
        <div className="grid gap-4">
          {routes.map((r) => {
            const isEditing = editingId === r.id;

            if (isEditing) {
              return (
                <div key={r.id} className="glass rounded-2xl p-5 space-y-4">
                  <div className="flex items-center justify-between">
                    <h3 className="text-sm font-semibold text-slate-900">Edit Route</h3>
                    <button onClick={() => setEditingId(null)} className="p-1 text-slate-400 hover:text-slate-600 cursor-pointer">
                      <X className="h-4 w-4" />
                    </button>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <input
                      placeholder="Name"
                      value={editForm.name ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <input
                      placeholder="Match Pattern"
                      value={editForm.match_pattern ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, match_pattern: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <select
                      value={editForm.target_provider ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, target_provider: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    >
                      <option value="">Select Provider</option>
                      {providers.map((p) => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                    <input
                      placeholder="Target Model (e.g. gpt-4o, or * for passthrough)"
                      value={editForm.target_model ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, target_model: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <select
                      value={editForm.fallback_provider ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, fallback_provider: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    >
                      <option value="">No Fallback Provider</option>
                      {providers.map((p) => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                    <input
                      placeholder="Fallback Model (optional)"
                      value={editForm.fallback_model ?? ""}
                      onChange={(e) => setEditForm({ ...editForm, fallback_model: e.target.value })}
                      className="rounded-xl border border-slate-200 bg-white px-4 py-2.5 text-sm outline-none focus:border-slate-400"
                    />
                    <div className="flex items-center gap-3">
                      <label className="text-sm text-slate-600">Active</label>
                      <input
                        type="checkbox"
                        checked={editForm.is_active ?? true}
                        onChange={(e) => setEditForm({ ...editForm, is_active: e.target.checked })}
                        className="h-4 w-4 rounded border-slate-300"
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <label className="text-sm text-slate-600">Priority</label>
                      <input
                        type="number"
                        min={0}
                        value={editForm.priority ?? 0}
                        onChange={(e) => setEditForm({ ...editForm, priority: parseInt(e.target.value) || 0 })}
                        className="w-20 rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm outline-none focus:border-slate-400"
                      />
                    </div>
                  </div>
                  <div className="flex gap-3">
                    <button
                      onClick={() => {
                        setEditError(null);
                        const input: UpdateRoutePayload = {
                          name: editForm.name || undefined,
                          match_pattern: editForm.match_pattern || undefined,
                          target_provider: editForm.target_provider || undefined,
                          target_model: editForm.target_model || undefined,
                          fallback_provider: editForm.fallback_provider || undefined,
                          fallback_model: editForm.fallback_model || undefined,
                          is_active: editForm.is_active,
                          priority: editForm.priority,
                        };
                        updateMut.mutate({ id: editForm.id, ...input });
                      }}
                      disabled={updateMut.isPending}
                      className="rounded-xl bg-slate-900 px-5 py-2 text-sm font-medium text-white hover:bg-slate-800 cursor-pointer disabled:opacity-50"
                    >
                      {updateMut.isPending ? "Saving..." : "Save"}
                    </button>
                    <button
                      onClick={() => { setEditingId(null); setEditError(null); }}
                      className="rounded-xl border border-slate-200 px-5 py-2 text-sm font-medium text-slate-600 hover:bg-slate-50 cursor-pointer"
                    >
                      Cancel
                    </button>
                  </div>
                  {editError && (
                    <p className="text-xs text-red-600 bg-red-50 rounded-lg px-3 py-2">{editError}</p>
                  )}
                </div>
              );
            }

            return (
              <div key={r.id} className="glass flex items-center justify-between rounded-2xl p-5">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="font-semibold text-slate-900">{r.name}</span>
                    <code className="rounded bg-slate-100 px-2 py-0.5 text-[11px] text-slate-600">
                      {r.match_pattern}
                    </code>
                    {!r.is_active && (
                      <span className="rounded-full bg-red-50 px-2 py-0.5 text-[10px] font-medium text-red-500">
                        Inactive
                      </span>
                    )}
                  </div>
                  <p className="mt-1 text-xs text-slate-500">
                    {providerName(r.target_provider)} → {r.target_model || "*"}
                    {r.fallback_model && ` (fallback: ${r.fallback_model})`}
                  </p>
                </div>
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => startEdit(r)}
                    className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-500 cursor-pointer"
                  >
                    <Pencil className="h-4 w-4" />
                  </button>
                  <button
                    onClick={() => deleteMut.mutate(r.id)}
                    className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500 cursor-pointer"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
