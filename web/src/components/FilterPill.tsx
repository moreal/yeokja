export function FilterPill(props: {
  label: string;
  value: string;
  current: string;
  onClick: (v: string) => void;
}) {
  const active = () => props.current === props.value;
  return (
    <button
      onClick={() => props.onClick(props.value)}
      class={`rounded-full px-3 py-1 text-xs font-medium ${
        active() ? "bg-blue-600 text-white" : "bg-gray-100 text-gray-700 hover:bg-gray-200"
      }`}
    >
      {props.label}
    </button>
  );
}
