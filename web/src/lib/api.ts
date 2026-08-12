const API_BASE = import.meta.env.VITE_API_BASE ?? "http://localhost:3000";

export interface Status {
  files: number;
  total_segments: number;
  translated: number;
  pending: number;
  stale: number;
  glossary_stale: number;
  context_changed: number;
}

export interface Segment {
  file: string;
  id: string;
  source: string;
  translation: string | null;
  status: string;
  issues: string[];
}

export interface TranslationJob {
  running: boolean;
  cancelled: boolean;
  started_at: string | null;
  finished_at: string | null;
  files_total: number;
  files_done: number;
  segments_total: number;
  segments_done: number;
  errors: string[];
}

export interface GlossaryTerm {
  term: string;
  translation: string;
}

export async function fetchStatus(): Promise<Status> {
  const res = await fetch(`${API_BASE}/api/status`);
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
  return res.json();
}

export async function fetchSegments(): Promise<Segment[]> {
  const res = await fetch(`${API_BASE}/api/segments`);
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
  return res.json();
}

export async function updateSegment(
  file: string,
  segmentId: string,
  translation: string,
): Promise<void> {
  const res = await fetch(
    `${API_BASE}/api/segments/${encodeURIComponent(file)}/${encodeURIComponent(segmentId)}`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ translation }),
    },
  );
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
}

export async function fetchGlossary(): Promise<GlossaryTerm[]> {
  const res = await fetch(`${API_BASE}/api/glossary`);
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
  return res.json();
}

export async function addGlossaryTerm(term: string, translation: string): Promise<void> {
  const res = await fetch(`${API_BASE}/api/glossary`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ term, translation }),
  });
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
}

export async function removeGlossaryTerm(term: string): Promise<void> {
  const res = await fetch(`${API_BASE}/api/glossary/${encodeURIComponent(term)}`, {
    method: "DELETE",
  });
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
}

export async function startTranslation(): Promise<void> {
  const res = await fetch(`${API_BASE}/api/translate/start`, { method: "POST" });
  // 409 means a run is already in progress; surface it as a normal state.
  if (!res.ok && res.status !== 409) throw new Error(`Failed: ${res.status}`);
}

export async function cancelTranslation(): Promise<void> {
  const res = await fetch(`${API_BASE}/api/translate/cancel`, { method: "POST" });
  // 409 means nothing is running; surface it as a normal state.
  if (!res.ok && res.status !== 409) throw new Error(`Failed: ${res.status}`);
}

export async function fetchTranslationJob(): Promise<TranslationJob> {
  const res = await fetch(`${API_BASE}/api/translate/status`);
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
  return res.json();
}

/// Subscribe to live translation progress (SSE). Returns a cleanup function.
export function subscribeTranslationEvents(onEvent: () => void): () => void {
  const source = new EventSource(`${API_BASE}/api/translate/events`);
  source.onmessage = onEvent;
  return () => source.close();
}

export interface EvaluationIssue {
  severity: "Error" | "Warning";
  kind: string;
  message: string;
}

export interface EvaluateResult {
  passed: boolean;
  issues: EvaluationIssue[];
}

export async function evaluateSegment(file: string, segmentId: string): Promise<EvaluateResult> {
  const res = await fetch(
    `${API_BASE}/api/segments/${encodeURIComponent(file)}/${encodeURIComponent(segmentId)}/evaluate`,
    { method: "POST" },
  );
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
  return res.json();
}
