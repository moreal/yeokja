export function StatCard(props: { label: string; value: number; color?: string }) {
  return (
    <div class="rounded-lg border border-gray-200 bg-white p-4 text-center">
      <div class={`text-2xl font-bold ${props.color ?? "text-gray-900"}`}>{props.value}</div>
      <div class="text-xs text-gray-500">{props.label}</div>
    </div>
  );
}
