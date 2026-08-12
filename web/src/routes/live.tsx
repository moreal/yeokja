import { createFileRoute } from "@tanstack/solid-router";
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { createStore, produce } from "solid-js/store";
import {
  fetchTranslationJob,
  subscribeProgressEvents,
  type BlockPhase,
  type ProgressEvent,
} from "../lib/api";
import { StatCard } from "../components/StatCard";

export const Route = createFileRoute("/live")({ component: LiveView });

/// A block as tracked by the client-side fold over the event stream.
interface LiveBlock {
  id: number;
  file: string;
  segments: number;
  source: string;
  attempt: number;
  phase: BlockPhase;
  startedAt: number;
  issues: string[];
}

interface FeedEntry {
  seq: number;
  at: number;
  kind: string;
  tone: "neutral" | "good" | "warn" | "bad";
  text: string;
}

/// Newest entries are kept at the head; older ones fall off the tail.
const FEED_LIMIT = 150;

interface LiveState {
  concurrency: number;
  queued: number;
  active: LiveBlock[];
  feed: FeedEntry[];
  blocksDone: number;
  segmentsDone: number;
  retried: number;
  connected: boolean;
  running: boolean;
}

function basename(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

function LiveView() {
  const [state, setState] = createStore<LiveState>({
    concurrency: 0,
    queued: 0,
    active: [],
    feed: [],
    blocksDone: 0,
    segmentsDone: 0,
    retried: 0,
    connected: false,
    running: false,
  });

  // Ticks once a second purely so elapsed times re-render.
  const [now, setNow] = createSignal(Date.now());
  let seq = 0;

  const push = (kind: string, tone: FeedEntry["tone"], text: string) => {
    setState(
      produce((s) => {
        s.feed.unshift({ seq: seq++, at: Date.now(), kind, tone, text });
        if (s.feed.length > FEED_LIMIT) s.feed.length = FEED_LIMIT;
      }),
    );
  };

  // Fold each event into state, the way a reducer folds actions into a store.
  const apply = (event: ProgressEvent) => {
    switch (event.type) {
      case "run_started":
        setState(
          produce((s) => {
            s.concurrency = event.concurrency;
            s.running = true;
            s.queued = 0;
            s.active = [];
            s.blocksDone = 0;
            s.segmentsDone = 0;
            s.retried = 0;
          }),
        );
        push("run", "neutral", `실행 시작 · 워커 ${event.concurrency}개`);
        break;

      case "files_discovered":
        push("run", "neutral", `파일 ${event.files.length}개 발견`);
        break;

      case "file_started":
        push("file", "neutral", `${basename(event.file)} 시작`);
        break;

      case "block_queued":
        setState("queued", (q) => q + 1);
        break;

      case "block_started":
        setState(
          produce((s) => {
            s.queued = Math.max(0, s.queued - 1);
            s.active.unshift({
              id: event.id,
              file: event.file,
              segments: event.segments,
              source: event.source,
              attempt: 1,
              phase: "translating",
              startedAt: Date.now(),
              issues: [],
            });
          }),
        );
        push(
          "start",
          "neutral",
          `#${event.id} ${basename(event.file)} · ${event.segments}문장 가져감`,
        );
        break;

      case "block_attempt": {
        let retry = false;
        setState(
          produce((s) => {
            const block = s.active.find((b) => b.id === event.id);
            if (!block) return;
            retry = event.attempt > block.attempt;
            block.attempt = event.attempt;
            block.phase = "translating";
            if (retry) s.retried += 1;
          }),
        );
        if (retry) {
          push("retry", "warn", `#${event.id} 재번역 시도 ${event.attempt}회차`);
        }
        break;
      }

      case "block_translating":
        setState(
          produce((s) => {
            const block = s.active.find((b) => b.id === event.id);
            if (block) block.phase = "evaluating";
          }),
        );
        break;

      case "block_evaluated": {
        setState(
          produce((s) => {
            const block = s.active.find((b) => b.id === event.id);
            if (block) block.issues = event.issues;
          }),
        );
        if (!event.passed) {
          push(
            "eval",
            "warn",
            `#${event.id} 평가 실패 (${event.attempt}회차) · ${event.issues.slice(0, 2).join(" / ") || "사유 없음"}`,
          );
        } else if (event.issues.length > 0) {
          push("eval", "neutral", `#${event.id} 통과 · 경고 ${event.issues.length}건`);
        }
        break;
      }

      case "block_translated":
        setState(
          produce((s) => {
            if (event.id !== null) s.active = s.active.filter((b) => b.id !== event.id);
            s.blocksDone += 1;
            s.segmentsDone += event.segments;
          }),
        );
        push(
          "done",
          "good",
          `#${event.id ?? "?"} 완료 · ${event.segments}문장 저장${
            event.current ? ` · ${event.current}` : ""
          }`,
        );
        break;

      case "block_failed":
        setState(
          produce((s) => {
            s.active = s.active.filter((b) => b.id !== event.id);
          }),
        );
        push("fail", "bad", `#${event.id} 실패 · ${event.error}`);
        break;

      case "file_completed":
        push("file", "good", `${basename(event.file)} 완료`);
        break;

      case "file_failed":
        push("fail", "bad", `${basename(event.file)} 실패 · ${event.error}`);
        break;

      case "cancelled":
        push("run", "warn", "취소됨 · 진행 중이던 블록은 저장 후 종료");
        break;

      case "finished":
        setState(
          produce((s) => {
            s.running = false;
            s.queued = 0;
            s.active = [];
          }),
        );
        push("run", event.errors > 0 ? "warn" : "good", `실행 종료 · 오류 ${event.errors}건`);
        break;
    }
  };

  onMount(() => {
    // Seed from the server snapshot so a mid-run page load is not blank; the
    // stream only carries events from the moment we connect.
    fetchTranslationJob()
      .then((job) => {
        setState(
          produce((s) => {
            s.concurrency = job.concurrency;
            s.queued = job.queued;
            s.running = job.running;
            s.retried = job.retried;
            s.segmentsDone = job.segments_done;
            s.active = job.active.map((b) => ({
              id: b.id,
              file: b.file,
              segments: b.segments,
              source: b.source,
              attempt: b.attempt,
              phase: b.phase,
              startedAt: new Date(b.started_at).getTime(),
              issues: [],
            }));
          }),
        );
      })
      .catch(() => {
        /* The stream still works; the snapshot is only a convenience. */
      });

    const unsubscribe = subscribeProgressEvents((event) => {
      setState("connected", true);
      apply(event);
    });
    const ticker = setInterval(() => setNow(Date.now()), 1000);

    onCleanup(() => {
      unsubscribe();
      clearInterval(ticker);
    });
  });

  const elapsed = (startedAt: number) => {
    const secs = Math.max(0, Math.round((now() - startedAt) / 1000));
    return secs < 60 ? `${secs}초` : `${Math.floor(secs / 60)}분 ${secs % 60}초`;
  };

  const idleSlots = () => Math.max(0, state.concurrency - state.active.length);

  const toneClass = (tone: FeedEntry["tone"]) =>
    tone === "good"
      ? "text-emerald-600"
      : tone === "warn"
        ? "text-amber-600"
        : tone === "bad"
          ? "text-red-600"
          : "text-gray-600";

  return (
    <main class="mx-auto max-w-6xl px-4 py-8">
      <div class="mb-6 flex items-center justify-between">
        <h2 class="text-2xl font-bold text-gray-900">Live</h2>
        <span
          class={`rounded-full px-3 py-1 text-xs font-medium ${
            state.running
              ? "bg-emerald-50 text-emerald-700"
              : state.connected
                ? "bg-gray-100 text-gray-500"
                : "bg-amber-50 text-amber-700"
          }`}
        >
          {state.running ? "실행 중" : state.connected ? "대기" : "연결 중…"}
        </span>
      </div>

      <div class="mb-6 grid grid-cols-2 gap-4 sm:grid-cols-5">
        <StatCard label="Workers" value={state.concurrency} />
        <StatCard label="Working" value={state.active.length} color="text-blue-600" />
        <StatCard label="Queued" value={state.queued} color="text-amber-600" />
        <StatCard label="Blocks Done" value={state.blocksDone} color="text-emerald-600" />
        <StatCard label="Retries" value={state.retried} color="text-violet-600" />
      </div>

      <section class="mb-6">
        <h3 class="mb-2 text-sm font-semibold text-gray-700">
          워커가 잡고 있는 블록 ({state.active.length}/{state.concurrency || "?"})
        </h3>
        <div class="space-y-2">
          <For
            each={state.active}
            fallback={
              <p class="rounded-lg border border-dashed border-gray-200 p-6 text-center text-sm text-gray-400">
                작업 중인 블록이 없습니다.
              </p>
            }
          >
            {(block) => (
              <div class="rounded-lg border border-gray-200 bg-white p-4">
                <div class="mb-2 flex flex-wrap items-center gap-2 text-xs">
                  <span class="font-mono text-gray-400">#{block.id}</span>
                  <span class="font-medium text-gray-700">{basename(block.file)}</span>
                  <span
                    class={`rounded-full px-2 py-0.5 font-medium ${
                      block.phase === "translating"
                        ? "bg-blue-50 text-blue-700"
                        : "bg-violet-50 text-violet-700"
                    }`}
                  >
                    {block.phase === "translating" ? "번역 중" : "평가 중"}
                  </span>
                  <Show when={block.attempt > 1}>
                    <span class="rounded-full bg-amber-50 px-2 py-0.5 font-medium text-amber-700">
                      {block.attempt}회차
                    </span>
                  </Show>
                  <span class="text-gray-400">
                    {block.segments}문장 · {elapsed(block.startedAt)}
                  </span>
                </div>
                <p class="line-clamp-3 text-sm text-gray-600">{block.source}</p>
                <Show when={block.issues.length > 0}>
                  <ul class="mt-2 list-inside list-disc text-xs text-amber-700">
                    <For each={block.issues.slice(0, 3)}>{(issue) => <li>{issue}</li>}</For>
                  </ul>
                </Show>
              </div>
            )}
          </For>
          <Show when={idleSlots() > 0 && state.running}>
            <p class="text-xs text-gray-400">유휴 워커 {idleSlots()}개</p>
          </Show>
        </div>
      </section>

      <section>
        <h3 class="mb-2 text-sm font-semibold text-gray-700">이벤트 피드</h3>
        <div class="max-h-96 overflow-y-auto rounded-lg border border-gray-200 bg-white">
          <For
            each={state.feed}
            fallback={<p class="p-6 text-center text-sm text-gray-400">아직 이벤트가 없습니다.</p>}
          >
            {(entry) => (
              <div class="flex gap-3 border-b border-gray-100 px-4 py-2 text-xs last:border-b-0">
                <span class="shrink-0 font-mono text-gray-400">
                  {new Date(entry.at).toLocaleTimeString("ko-KR", { hour12: false })}
                </span>
                <span class="w-12 shrink-0 font-medium text-gray-400">{entry.kind}</span>
                <span class={`truncate ${toneClass(entry.tone)}`}>{entry.text}</span>
              </div>
            )}
          </For>
        </div>
      </section>
    </main>
  );
}
