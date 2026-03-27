import { createFileRoute } from "@tanstack/solid-router";
import { createResource, createSignal, For, Show } from "solid-js";
import { fetchSegments, updateSegment } from "../lib/api";
import type { Segment } from "../lib/api";
import { StatusBadge } from "../components/StatusBadge";
import { FilterPill } from "../components/FilterPill";

export const Route = createFileRoute("/segments")({ component: Segments });

function Segments() {
  const [segments, { refetch }] = createResource(fetchSegments);
  const [filter, setFilter] = createSignal("all");
  const [editKey, setEditKey] = createSignal<string | null>(null);
  const [editValue, setEditValue] = createSignal("");

  const filtered = () => {
    const segs = segments() ?? [];
    const f = filter();
    return f === "all" ? segs : segs.filter((s) => s.status === f);
  };

  const statuses = () => {
    const counts: Record<string, number> = {};
    for (const s of segments() ?? []) {
      counts[s.status] = (counts[s.status] ?? 0) + 1;
    }
    return counts;
  };

  const startEdit = (seg: Segment) => {
    setEditKey(`${seg.file}::${seg.id}`);
    setEditValue(seg.translation ?? "");
  };

  const saveEdit = async (file: string, id: string) => {
    await updateSegment(file, id, editValue());
    setEditKey(null);
    refetch();
  };

  return (
    <main class="mx-auto max-w-6xl px-4 py-8">
      <h2 class="mb-4 text-2xl font-bold text-gray-900">Segments</h2>

      <div class="mb-4 flex flex-wrap gap-2">
        <FilterPill label="All" value="all" current={filter()} onClick={setFilter} />
        <For each={Object.entries(statuses())}>
          {([status, count]) => (
            <FilterPill
              label={`${status} (${count})`}
              value={status}
              current={filter()}
              onClick={setFilter}
            />
          )}
        </For>
      </div>

      <Show when={segments.error}>
        <div class="rounded-lg bg-red-50 p-4 text-sm text-red-700">Cannot connect to server.</div>
      </Show>

      <Show when={segments()} fallback={<p class="text-gray-500">Loading...</p>}>
        <div class="overflow-x-auto rounded-lg border border-gray-200">
          <table class="w-full text-sm">
            <thead class="border-b border-gray-200 bg-gray-50 text-left text-xs uppercase text-gray-500">
              <tr>
                <th class="px-4 py-3">File</th>
                <th class="px-4 py-3">Source</th>
                <th class="px-4 py-3">Translation</th>
                <th class="px-4 py-3">Status</th>
                <th class="px-4 py-3 w-24"></th>
              </tr>
            </thead>
            <tbody>
              <For each={filtered()}>
                {(seg) => {
                  const key = () => `${seg.file}::${seg.id}`;
                  const editing = () => editKey() === key();
                  return (
                    <tr class="border-b border-gray-100 hover:bg-gray-50">
                      <td class="max-w-[120px] truncate px-4 py-3 text-xs text-gray-400">
                        {seg.file}
                      </td>
                      <td class="max-w-[300px] px-4 py-3">{seg.source}</td>
                      <td class="max-w-[300px] px-4 py-3">
                        <Show
                          when={editing()}
                          fallback={
                            <span class={seg.translation ? "" : "italic text-gray-400"}>
                              {seg.translation ?? "(untranslated)"}
                            </span>
                          }
                        >
                          <textarea
                            value={editValue()}
                            onInput={(e) => setEditValue(e.currentTarget.value)}
                            class="w-full rounded border border-gray-300 p-1.5 text-sm"
                            rows={3}
                          />
                        </Show>
                      </td>
                      <td class="px-4 py-3">
                        <StatusBadge status={seg.status} />
                      </td>
                      <td class="px-4 py-3">
                        <Show
                          when={editing()}
                          fallback={
                            <button
                              onClick={() => startEdit(seg)}
                              class="rounded bg-gray-100 px-3 py-1 text-xs hover:bg-gray-200"
                            >
                              Edit
                            </button>
                          }
                        >
                          <div class="flex gap-1">
                            <button
                              onClick={() => saveEdit(seg.file, seg.id)}
                              class="rounded bg-emerald-500 px-3 py-1 text-xs text-white hover:bg-emerald-600"
                            >
                              Save
                            </button>
                            <button
                              onClick={() => setEditKey(null)}
                              class="rounded bg-gray-200 px-3 py-1 text-xs hover:bg-gray-300"
                            >
                              Cancel
                            </button>
                          </div>
                        </Show>
                      </td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>
        </div>
      </Show>
    </main>
  );
}
