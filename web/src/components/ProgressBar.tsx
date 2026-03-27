export function ProgressBar(props: { percentage: string }) {
  return (
    <div class="mb-6 rounded-xl bg-gray-50 p-6">
      <div class="mb-2 text-3xl font-bold text-gray-900">{props.percentage}% Translated</div>
      <div class="h-2 overflow-hidden rounded-full bg-gray-200">
        <div
          class="h-full rounded-full bg-emerald-500 transition-all duration-300"
          style={{ width: `${props.percentage}%` }}
        />
      </div>
    </div>
  );
}
