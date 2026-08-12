import { createFileRoute } from "@tanstack/solid-router";
import { onCleanup, onMount, Show } from "solid-js";
import {
  cancelTranslation,
  fetchStatus,
  fetchTranslationJob,
  startTranslation,
  subscribeTranslationEvents,
} from "../lib/api";
import { usePolling } from "../hooks/usePolling";
import { StatCard } from "../components/StatCard";
import { ProgressBar } from "../components/ProgressBar";

export const Route = createFileRoute("/")({ component: Dashboard });

function Dashboard() {
  const { data: status, refetch, lastPolled, countdown } = usePolling(fetchStatus);
  const { data: job, refetch: refetchJob } = usePolling(fetchTranslationJob);

  // Live progress: every SSE event refreshes the job snapshot; polling stays
  // as fallback when the stream is unavailable.
  onMount(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribeTranslationEvents(() => {
      refetchJob();
      // Debounce the heavier full-status refresh.
      if (timer) clearTimeout(timer);
      timer = setTimeout(refetch, 500);
    });
    onCleanup(() => {
      if (timer) clearTimeout(timer);
      unsubscribe();
    });
  });

  const handleStart = async () => {
    await startTranslation();
    refetchJob();
  };

  const handleCancel = async () => {
    await cancelTranslation();
    refetchJob();
  };

  const formatTime = (date: Date) => {
    return date.toLocaleTimeString("ko-KR", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  };

  return (
    <main class="mx-auto max-w-6xl px-4 py-8">
      <div class="mb-6 flex items-center justify-between">
        <h2 class="text-2xl font-bold text-gray-900">Dashboard</h2>
        <div class="flex items-center gap-3">
          <Show when={lastPolled()}>
            {(time) => (
              <span class="text-xs text-gray-400">
                {formatTime(time())} 기준 · {countdown()}초 후 갱신
              </span>
            )}
          </Show>
          <button
            onClick={handleStart}
            disabled={job()?.running}
            class="rounded-lg bg-emerald-600 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-700 disabled:cursor-not-allowed disabled:bg-gray-300"
          >
            {job()?.running ? "Translating..." : "Start Translation"}
          </button>
          <Show when={job()?.running}>
            <button
              onClick={handleCancel}
              disabled={job()?.cancelled}
              class="rounded-lg bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700 disabled:cursor-not-allowed disabled:bg-gray-300"
            >
              {job()?.cancelled ? "Cancelling..." : "Cancel"}
            </button>
          </Show>
          <button
            onClick={refetch}
            class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
          >
            Refresh
          </button>
        </div>
      </div>

      <Show when={job()} keyed>
        {(j) => (
          <Show when={j.running || j.errors.length > 0}>
            <div class="mb-4 rounded-lg border border-gray-200 bg-white p-4 text-sm">
              <Show when={j.running}>
                <p class="text-gray-700">
                  번역 진행 중: 파일 {j.files_done}/{j.files_total} · 세그먼트 {j.segments_done}/
                  {j.segments_total}
                </p>
              </Show>
              <Show when={j.errors.length > 0}>
                <ul class="mt-1 list-inside list-disc text-red-600">
                  {j.errors.map((e) => (
                    <li>{e}</li>
                  ))}
                </ul>
              </Show>
            </div>
          </Show>
        )}
      </Show>

      <Show when={status.error}>
        <div class="mb-4 rounded-lg bg-red-50 p-4 text-sm text-red-700">
          Cannot connect to server. Is it running on port 3000?
        </div>
      </Show>

      <Show when={status()} fallback={<p class="text-gray-500">Loading...</p>}>
        {(s) => {
          const pct = () =>
            s().total_segments > 0
              ? ((s().translated / s().total_segments) * 100).toFixed(1)
              : "0.0";

          return (
            <>
              <ProgressBar percentage={pct()} />

              <div class="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-4 lg:grid-cols-7">
                <StatCard label="Files" value={s().files} />
                <StatCard label="Total" value={s().total_segments} />
                <StatCard label="Translated" value={s().translated} color="text-emerald-600" />
                <StatCard label="Pending" value={s().pending} color="text-amber-600" />
                <StatCard label="Stale" value={s().stale} color="text-red-600" />
                <StatCard label="Glossary Stale" value={s().glossary_stale} color="text-orange-600" />
                <StatCard label="Context Changed" value={s().context_changed} color="text-violet-600" />
              </div>
            </>
          );
        }}
      </Show>
    </main>
  );
}
