from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
FINGERPRINT_SCRIPT = REPOSITORY_ROOT / ".github" / "scripts" / "build-fingerprint.sh"


def run_git(args: list[str], cwd: Path) -> None:
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True)


class BuildFingerprintTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)

        # The script only ever runs from inside a checkout, so the root
        # itself is the git repo `git ls-files` resolves paths against.
        run_git(["init", "-q"], self.root)
        run_git(["config", "user.email", "test@example.com"], self.root)
        run_git(["config", "user.name", "Test"], self.root)

        self.project = self.root / "projects" / "sample"
        self.project.mkdir(parents=True)
        # Real projects gitignore ko/ (it's reconstructed from state/ on every
        # run) — without this, a later `git add -A` in a test would pick it
        # up as newly tracked and shift the fingerprint on its own.
        self.write(self.project / ".gitignore", "ko/\n")
        self.write(self.project / "yeokja.toml", "[project]\n")
        self.state_dir = self.project / "state"
        self.state_dir.mkdir()
        self.write(self.state_dir / "ch1.md.yeokja.json", '{"issues": []}')

        self.upstream = self.project / "upstream"
        self.upstream.mkdir()
        run_git(["init", "-q"], self.upstream)
        run_git(["config", "user.email", "test@example.com"], self.upstream)
        run_git(["config", "user.name", "Test"], self.upstream)
        self.write(self.upstream / "book.md", "hello")
        run_git(["add", "-A"], self.upstream)
        run_git(["commit", "-q", "-m", "upstream initial"], self.upstream)

        # A single commit that already tracks upstream as an embedded repo
        # (a stand-in for the real submodule gitlink) alongside the project
        # files, so later commits don't shift the tracked-file set on their
        # own just by noticing upstream for the first time.
        run_git(["add", "-A"], self.root)
        run_git(["commit", "-q", "-m", "initial"], self.root)

        self.ko = self.project / "ko"
        self.ko.mkdir()
        self.write(self.ko / "book.md", "안녕하세요")

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    @staticmethod
    def write(path: Path, contents: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")

    def fingerprint(self, target: str = "html", daily: bool = False, env: dict | None = None) -> str:
        self.assertTrue(FINGERPRINT_SCRIPT.is_file(), f"missing script: {FINGERPRINT_SCRIPT}")
        args = ["bash", str(FINGERPRINT_SCRIPT), str(self.project), target]
        if daily:
            args.append("--daily")
        full_env = dict(os.environ)
        if env:
            full_env.update(env)
        result = subprocess.run(
            args, cwd=self.root, check=False, capture_output=True, text=True, env=full_env
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        fingerprint = result.stdout.strip()
        self.assertRegex(fingerprint, r"^[0-9a-f]{16}$")
        return fingerprint

    def test_same_inputs_produce_the_same_fingerprint(self) -> None:
        self.assertEqual(self.fingerprint(), self.fingerprint())

    def test_different_targets_produce_different_fingerprints(self) -> None:
        self.assertNotEqual(self.fingerprint("html"), self.fingerprint("pdf"))

    def test_changing_ko_output_changes_the_fingerprint(self) -> None:
        before = self.fingerprint()
        self.write(self.ko / "book.md", "changed translation")
        self.assertNotEqual(before, self.fingerprint())

    def test_changing_a_tracked_project_file_changes_the_fingerprint(self) -> None:
        before = self.fingerprint()
        self.write(self.project / "yeokja.toml", "[project]\nextra = true\n")
        run_git(["add", "-A"], self.root)
        run_git(["commit", "-q", "-m", "tweak config"], self.root)
        self.assertNotEqual(before, self.fingerprint())

    def test_changing_only_state_leaves_the_fingerprint_unchanged(self) -> None:
        before = self.fingerprint()
        self.write(self.state_dir / "ch1.md.yeokja.json", '{"issues": ["something"]}')
        run_git(["add", "-A"], self.root)
        run_git(["commit", "-q", "-m", "state churn"], self.root)
        self.assertEqual(before, self.fingerprint())

    def test_changing_the_upstream_commit_changes_the_fingerprint(self) -> None:
        before = self.fingerprint()
        self.write(self.upstream / "book.md", "hello again")
        run_git(["add", "-A"], self.upstream)
        run_git(["commit", "-q", "-m", "upstream update"], self.upstream)
        self.assertNotEqual(before, self.fingerprint())

    def test_dirty_upstream_changes_the_fingerprint(self) -> None:
        before = self.fingerprint()
        self.write(self.upstream / "book.md", "hello, uncommitted")
        self.assertNotEqual(before, self.fingerprint())

    def test_daily_mode_changes_with_the_injected_date(self) -> None:
        first = self.fingerprint(daily=True, env={"YEOKJA_FINGERPRINT_DATE": "2026-01-01"})
        second = self.fingerprint(daily=True, env={"YEOKJA_FINGERPRINT_DATE": "2026-01-02"})
        self.assertNotEqual(first, second)

    def test_daily_mode_is_stable_for_the_same_injected_date(self) -> None:
        first = self.fingerprint(daily=True, env={"YEOKJA_FINGERPRINT_DATE": "2026-01-01"})
        second = self.fingerprint(daily=True, env={"YEOKJA_FINGERPRINT_DATE": "2026-01-01"})
        self.assertEqual(first, second)

    def test_missing_ko_directory_does_not_fail(self) -> None:
        for entry in self.ko.iterdir():
            entry.unlink()
        self.ko.rmdir()
        self.fingerprint()  # asserts return code 0 internally


if __name__ == "__main__":
    unittest.main()
