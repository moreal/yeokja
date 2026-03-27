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

export async function startTranslation(): Promise<void> {
  const res = await fetch(`${API_BASE}/api/translate/start`, { method: "POST" });
  if (!res.ok) throw new Error(`Failed: ${res.status}`);
}
