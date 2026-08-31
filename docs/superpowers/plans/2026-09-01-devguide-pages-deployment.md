# Devguide GitHub Pages Deployment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish the committed Python Developer's Guide Korean translation at `/devguide/` while retaining the last deployed version of any legacy project whose current rebuild fails.

**Architecture:** The Pages stage starts from a mirror of the currently published site, then a tested shell helper replaces only subtrees backed by artifacts from successful jobs. The existing rebuild matrix gains a required devguide entry and treats legacy failures as non-blocking; missing legacy artifacts preserve their mirrored subtrees, while a missing devguide artifact fails staging.

**Tech Stack:** GitHub Actions YAML, POSIX shell/Bash, Python 3 `unittest`, yeokja CLI, Sphinx, `uv`, GitHub Pages Actions.

**Spec:** `docs/superpowers/specs/2026-09-01-devguide-pages-deployment-design.md`

## Global Constraints

- Work directly on `main`, as explicitly requested by the user.
- Push only after local tests, devguide reconstruction, warning-as-error build, and HTML audit pass.
- Never call a translation provider in Actions; reconstruction must consume the committed complete `projects/devguide/state/` tree.
- A devguide failure must block deployment.
- A legacy project failure must preserve its currently published subtree instead of deleting it.
- Deletion targets in the staging helper must be fixed project paths, never matrix or external input.
- Every created commit contains exactly one `Assisted-by: Codex:gpt-5.6-sol` trailer.

---

### Task 1: Tested Pages artifact overlay helper

**Files:**
- Create: `.github/scripts/test_stage_pages.py`
- Create: `.github/scripts/stage-pages.sh`

**Interfaces:**
- Consumes: `stage-pages.sh <artifacts-dir> <site-dir> <landing-dir>` where the site directory already contains the mirrored deployed baseline.
- Produces: a complete site tree containing required `devguide/`, refreshed root landing files, successful artifact overlays, and untouched baseline directories for absent legacy artifacts.

- [ ] **Step 1: Write failing tests for required inputs and overlay behavior**

Create a standard-library `unittest` module that builds temporary `artifacts`, `site`, and `landing` trees, invokes `.github/scripts/stage-pages.sh`, and asserts:

```python
def test_missing_devguide_artifact_fails(self):
    result = self.run_stage()
    self.assertNotEqual(result.returncode, 0)
    self.assertIn("required devguide artifact", result.stderr)

def test_missing_legacy_artifact_preserves_published_tree(self):
    self.write(self.site / "mil" / "old.html", "published")
    self.add_site_artifact("dist-devguide", "devguide")
    result = self.run_stage()
    self.assertEqual(result.returncode, 0, result.stderr)
    self.assertEqual((self.site / "mil" / "old.html").read_text(), "published")

def test_successful_artifact_replaces_old_subtree(self):
    self.write(self.site / "mil" / "old.html", "old")
    self.add_site_artifact("dist-mil", "new")
    self.add_site_artifact("dist-devguide", "devguide")
    result = self.run_stage()
    self.assertEqual(result.returncode, 0, result.stderr)
    self.assertFalse((self.site / "mil" / "old.html").exists())
    self.assertEqual((self.site / "mil" / "index.html").read_text(), "new")
```

Add separate tests for a missing baseline `index.html`, paired PyPy/RPython overlays, Napkin HTML/PDF/EPUB overlays, and root `index.html`/`favicon.svg` refresh.

- [ ] **Step 2: Run tests and verify RED**

Run:

```bash
python3 .github/scripts/test_stage_pages.py -v
```

Expected: FAIL because `.github/scripts/stage-pages.sh` does not exist.

- [ ] **Step 3: Implement the minimal fixed-path staging helper**

Implement a Bash script with `set -euo pipefail`, exactly three directory arguments, and fixed helper calls such as:

```bash
overlay_site() {
  artifact_name=$1
  source_name=$2
  destination_name=$3
  source_path="$artifacts_dir/$artifact_name/$source_name"
  if [ ! -d "$source_path" ]; then
    echo "warning: preserving published $destination_name; $artifact_name is unavailable" >&2
    return
  fi
  rm -rf "$site_dir/$destination_name"
  cp -R "$source_path" "$site_dir/$destination_name"
}

test -s "$site_dir/index.html" || fail "published Pages baseline is missing index.html"
test -s "$artifacts_dir/dist-devguide/site/index.html" || fail "required devguide artifact is missing index.html"
```

Call the helper for each existing fixed project mapping and for required `dist-devguide/site -> devguide`. Copy Napkin PDF/EPUB only when their exact files exist, then copy `landing/index.html` and `landing/favicon.svg` to the site root.

- [ ] **Step 4: Run focused tests and shellcheck**

Run:

```bash
python3 .github/scripts/test_stage_pages.py -v
shellcheck .github/scripts/stage-pages.sh
```

Expected: all staging tests pass and ShellCheck exits 0.

- [ ] **Step 5: Commit the helper**

```bash
git add .github/scripts/stage-pages.sh .github/scripts/test_stage_pages.py
git commit -m "ci: preserve Pages sites across partial rebuilds" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 2: Required devguide build and Pages integration

**Files:**
- Modify: `.github/workflows/pages.yml`
- Modify: `site/index.html`

**Interfaces:**
- Consumes: Task 1's `stage-pages.sh <artifacts-dir> <site-dir> <landing-dir>` command and `dist-devguide/site` artifact layout.
- Produces: a `main` push workflow that builds devguide as the required matrix member, preserves failed legacy sites, stages `/devguide`, and links it from the root page.

- [ ] **Step 1: Add devguide to the rebuild matrix**

Modify `.github/workflows/pages.yml` to add:

```yaml
- project: devguide
  target: html
  toolchain: python3-devguide
  unshallow: true
  artifact: dist-devguide
  artifact_path: projects/devguide/dist
```

Set job-level conditional failure handling:

```yaml
continue-on-error: ${{ matrix.project != 'devguide' }}
```

Include `python3-devguide` in the Python setup condition, add conditional `astral-sh/setup-uv@v10.0.1`, fetch the devguide submodule's full history for Sphinx Git timestamps, exclude devguide from the legacy per-state reconstruction step, and add a devguide-specific whole-project reconstruction/status step. `v10.0.1` is the exact current official release verified before implementation; the repository does not expose a moving `v10` tag.

- [ ] **Step 2: Replace direct staging with baseline preservation and the helper**

Before downloading current-run artifacts, mirror the current Pages tree using fixed-origin `wget` options and require `_site/index.html`:

```bash
mkdir -p _site
wget --mirror --no-host-directories --cut-dirs=1 --no-parent \
  --directory-prefix=_site https://moreal.github.io/yeokja/ || true
test -s _site/index.html
```

Download available `dist-*` artifacts, then run:

```bash
.github/scripts/stage-pages.sh artifacts _site site
```

Do not retain the old unconditional `cp` list.

- [ ] **Step 3: Add the devguide landing-page entry**

Add a `site/index.html` list item linking to `devguide/`, describing it as the unofficial Korean Python Developer's Guide translation, linking to `python/devguide`, and showing a `CC0-1.0` tag.

- [ ] **Step 4: Run focused and syntax checks**

Run:

```bash
python3 .github/scripts/test_stage_pages.py -v
shellcheck .github/scripts/stage-pages.sh
ruby -e 'require "yaml"; YAML.parse_file(".github/workflows/pages.yml")'
actionlint -ignore 'label "yeokja" is unknown' -ignore 'SC2044' \
  .github/workflows/pages.yml
git diff --check
```

Expected: the real staging behavior tests pass, shell syntax is clean, the workflow parses as YAML, actionlint v1.7.12 reports no branch-introduced issue after excluding the existing custom-runner and legacy loop diagnostics, and the diff has no whitespace errors. Workflow behavior itself is verified by the production Actions run in Task 3 rather than by brittle source-text assertions.

- [ ] **Step 5: Run project-wide and devguide gates**

Run:

```bash
cargo test --workspace
python3 -m unittest discover -s projects/devguide/scripts -p 'test_*.py' -v
./target/debug/yeokja -C projects/devguide translate upstream
./target/debug/yeokja -C projects/devguide status --check upstream
./target/debug/yeokja -C projects/devguide evaluate --mechanical-only upstream
./target/debug/yeokja -C projects/devguide build html
python3 projects/devguide/scripts/audit.py translation
python3 projects/devguide/scripts/audit.py html
```

Expected: 491 Rust tests and all current devguide Python tests pass; 64 files and 5,341 segments are complete; mechanical evaluation has 0 issues; warning-as-error build and both audits pass.

- [ ] **Step 6: Commit the workflow integration**

```bash
git add .github/workflows/pages.yml site/index.html
git commit -m "ci: deploy the Korean devguide to Pages" \
  -m "Assisted-by: Codex:gpt-5.6-sol"
```

---

### Task 3: Push and production verification

**Files:**
- Verify only; no source changes expected.

**Interfaces:**
- Consumes: the complete `main` branch from Tasks 1–2.
- Produces: a successful GitHub Pages deployment and a public `/devguide/` site.

- [ ] **Step 1: Verify commit and worktree hygiene**

Run:

```bash
git status --short --branch --untracked-files=all
git log -3 --format='%H%n%B'
```

Expected: clean `main`; every new commit has exactly one required assistance trailer.

- [ ] **Step 2: Push main**

Use the authenticated HTTPS GitHub credential helper if the configured SSH agent remains unavailable:

```bash
git -c credential.helper='!gh auth git-credential' push \
  https://github.com/moreal/yeokja.git main:main
```

- [ ] **Step 3: Monitor the Pages workflow**

Find the run for the pushed HEAD and wait for completion:

```bash
gh run list --workflow pages.yml --branch main --limit 1 \
  --json databaseId,headSha,status,conclusion,url
gh run watch <run-id> --exit-status
```

Expected: build-cli, required devguide rebuild, stage, and deploy succeed. Legacy failures may appear as non-blocking continued jobs.

- [ ] **Step 4: Verify the public deployment**

Run:

```bash
curl -fsSL https://moreal.github.io/yeokja/devguide/ > /tmp/devguide-pages.html
grep -q 'Python 개발자 가이드' /tmp/devguide-pages.html
curl -fsSL https://moreal.github.io/yeokja/ | grep -q 'href="devguide/"'
```

Expected: both URLs return success, `/devguide/` contains its Korean title, and the root page links to it.
