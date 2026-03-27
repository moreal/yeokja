const colors: Record<string, string> = {
  Translated: "bg-emerald-100 text-emerald-800",
  Pending: "bg-amber-100 text-amber-800",
  Stale: "bg-red-100 text-red-800",
  GlossaryStale: "bg-orange-100 text-orange-800",
  ContextChanged: "bg-violet-100 text-violet-800",
};

export function StatusBadge(props: { status: string }) {
  return (
    <span
      class={`rounded-full px-2.5 py-0.5 text-xs font-medium ${colors[props.status] ?? "bg-gray-100 text-gray-700"}`}
    >
      {props.status}
    </span>
  );
}
