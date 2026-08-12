import { createFileRoute } from "@tanstack/solid-router";
import { createResource, createSignal, For, Show } from "solid-js";
import { fetchGlossary, addGlossaryTerm, removeGlossaryTerm } from "../lib/api";

export const Route = createFileRoute("/glossary")({ component: Glossary });

function Glossary() {
  const [glossary, { refetch }] = createResource(fetchGlossary);
  const [term, setTerm] = createSignal("");
  const [translation, setTranslation] = createSignal("");

  const handleAdd = async (e: Event) => {
    e.preventDefault();
    const t = term().trim();
    const tr = translation().trim();
    if (!t || !tr) return;
    await addGlossaryTerm(t, tr);
    setTerm("");
    setTranslation("");
    refetch();
  };

  return (
    <main class="mx-auto max-w-3xl px-4 py-8">
      <h2 class="mb-6 text-2xl font-bold text-gray-900">Glossary</h2>

      <form onSubmit={handleAdd} class="mb-6 flex items-end gap-3">
        <div>
          <label class="mb-1 block text-sm text-gray-600">Term</label>
          <input
            type="text"
            value={term()}
            onInput={(e) => setTerm(e.currentTarget.value)}
            placeholder="e.g. repository"
            class="rounded-lg border border-gray-300 px-3 py-2 text-sm"
          />
        </div>
        <div>
          <label class="mb-1 block text-sm text-gray-600">Translation</label>
          <input
            type="text"
            value={translation()}
            onInput={(e) => setTranslation(e.currentTarget.value)}
            placeholder="e.g. 저장소"
            class="rounded-lg border border-gray-300 px-3 py-2 text-sm"
          />
        </div>
        <button
          type="submit"
          class="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700"
        >
          Add
        </button>
      </form>

      <Show when={glossary.error}>
        <div class="rounded-lg bg-red-50 p-4 text-sm text-red-700">Cannot connect to server.</div>
      </Show>

      <Show when={glossary()} fallback={<p class="text-gray-500">Loading...</p>}>
        {(terms) => (
          <Show
            when={terms().length > 0}
            fallback={<p class="text-gray-500">No glossary terms defined.</p>}
          >
            <div class="overflow-hidden rounded-lg border border-gray-200">
              <table class="w-full text-sm">
                <thead class="border-b border-gray-200 bg-gray-50 text-left text-xs uppercase text-gray-500">
                  <tr>
                    <th class="px-4 py-3">Term</th>
                    <th class="px-4 py-3">Translation</th>
                    <th class="w-16 px-4 py-3"></th>
                  </tr>
                </thead>
                <tbody>
                  <For each={terms()}>
                    {(t) => (
                      <tr class="border-b border-gray-100">
                        <td class="px-4 py-3 font-medium">{t.term}</td>
                        <td class="px-4 py-3">{t.translation}</td>
                        <td class="px-4 py-3 text-right">
                          <button
                            onClick={async () => {
                              await removeGlossaryTerm(t.term);
                              refetch();
                            }}
                            class="text-xs text-red-500 hover:text-red-700"
                            title="Remove term"
                          >
                            Delete
                          </button>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>
        )}
      </Show>
    </main>
  );
}
