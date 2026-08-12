import { Link } from "@tanstack/solid-router";

export default function Header() {
  return (
    <header class="border-b border-gray-200 bg-white">
      <nav class="mx-auto flex max-w-6xl items-center gap-6 px-4 py-3">
        <Link to="/" class="text-lg font-bold tracking-tight text-gray-900">
          Yeokja
        </Link>
        <div class="flex items-center gap-4 text-sm font-medium">
          <Link
            to="/"
            class="text-gray-600 hover:text-gray-900"
            activeProps={{ class: "text-blue-600" }}
            activeOptions={{ exact: true }}
          >
            Dashboard
          </Link>
          <Link
            to="/live"
            class="text-gray-600 hover:text-gray-900"
            activeProps={{ class: "text-blue-600" }}
          >
            Live
          </Link>
          <Link
            to="/segments"
            class="text-gray-600 hover:text-gray-900"
            activeProps={{ class: "text-blue-600" }}
          >
            Segments
          </Link>
          <Link
            to="/glossary"
            class="text-gray-600 hover:text-gray-900"
            activeProps={{ class: "text-blue-600" }}
          >
            Glossary
          </Link>
        </div>
      </nav>
    </header>
  );
}
