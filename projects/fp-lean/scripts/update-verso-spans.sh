#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
upstream_root="$project_root/upstream"
book_root="$upstream_root/book"
extractor="$project_root/tools/VersoSpans.lean"
output="$project_root/verso-spans.json"
temporary="$project_root/.verso-spans.json.tmp"

verso_revision="$(python3 -c '
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    manifest = json.load(handle)
packages = manifest.get("packages", manifest) if isinstance(manifest, dict) else manifest
for package in packages:
    if package.get("name") == "verso":
        print(package["rev"])
        break
else:
    raise SystemExit("verso is absent from lake-manifest.json")
' "$book_root/lake-manifest.json")"

sources=("book/FPLean.lean")
while IFS= read -r source; do
  sources+=("${source#"$upstream_root/"}")
done < <(find "$upstream_root/book/FPLean" -type f -name '*.lean' | LC_ALL=C sort)
# The example modules carry the equational-step justifications that
# `anchorEqSteps` blocks render; the book payload must match them line by line.
while IFS= read -r source; do
  sources+=("${source#"$upstream_root/"}")
done < <(find "$upstream_root/examples/Examples" -type f -name '*.lean' | LC_ALL=C sort)

cleanup() {
  rm -f "$temporary"
}
trap cleanup EXIT

(
  cd "$book_root"
  lake build Verso
  lake env lean --run "$extractor" \
    "$temporary" \
    "$verso_revision" \
    upstream/book/lake-manifest.json \
    "$upstream_root" \
    upstream \
    "${sources[@]}"
)

mv "$temporary" "$output"
trap - EXIT
echo "Wrote $output (${#sources[@]} Lean source files, Verso $verso_revision)"
