from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
STAGE_SCRIPT = REPOSITORY_ROOT / ".github" / "scripts" / "stage-pages.sh"


class StagePagesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        self.artifacts = self.root / "artifacts"
        self.site = self.root / "site"
        self.landing = self.root / "landing"
        self.artifacts.mkdir()
        self.site.mkdir()
        self.landing.mkdir()
        self.write(self.site / "index.html", "published root")
        self.write(self.landing / "index.html", "new root")
        self.write(self.landing / "favicon.svg", "new favicon")

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    @staticmethod
    def write(path: Path, contents: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")

    def add_site_artifact(
        self, artifact: str, contents: str, source_directory: str = "site"
    ) -> None:
        self.write(
            self.artifacts / artifact / source_directory / "index.html", contents
        )

    def add_devguide(self) -> None:
        self.add_site_artifact("dist-devguide", "devguide")

    def run_stage(self) -> subprocess.CompletedProcess[str]:
        self.assertTrue(STAGE_SCRIPT.is_file(), f"missing script: {STAGE_SCRIPT}")
        return subprocess.run(
            [
                "bash",
                str(STAGE_SCRIPT),
                str(self.artifacts),
                str(self.site),
                str(self.landing),
            ],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_missing_published_baseline_index_fails(self) -> None:
        (self.site / "index.html").unlink()
        self.add_devguide()

        result = self.run_stage()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("published Pages baseline is missing index.html", result.stderr)

    def test_missing_devguide_artifact_fails(self) -> None:
        result = self.run_stage()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("required devguide artifact is missing index.html", result.stderr)

    def test_missing_legacy_artifact_preserves_published_tree(self) -> None:
        self.write(self.site / "mil" / "old.html", "published")
        self.add_devguide()

        result = self.run_stage()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            (self.site / "mil" / "old.html").read_text(encoding="utf-8"),
            "published",
        )

    def test_successful_artifact_replaces_old_subtree(self) -> None:
        self.write(self.site / "mil" / "old.html", "old")
        self.add_site_artifact("dist-mil", "new")
        self.add_devguide()

        result = self.run_stage()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.site / "mil" / "old.html").exists())
        self.assertEqual(
            (self.site / "mil" / "index.html").read_text(encoding="utf-8"),
            "new",
        )

    def test_pypy_artifact_replaces_pypy_and_rpython_together(self) -> None:
        self.write(self.site / "pypy" / "old.html", "old pypy")
        self.write(self.site / "rpython" / "old.html", "old rpython")
        self.add_site_artifact("dist-pypy", "new pypy")
        self.add_site_artifact("dist-pypy", "new rpython", "rpython-site")
        self.add_devguide()

        result = self.run_stage()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.site / "pypy" / "old.html").exists())
        self.assertFalse((self.site / "rpython" / "old.html").exists())
        self.assertEqual(
            (self.site / "pypy" / "index.html").read_text(encoding="utf-8"),
            "new pypy",
        )
        self.assertEqual(
            (self.site / "rpython" / "index.html").read_text(encoding="utf-8"),
            "new rpython",
        )

    def test_napkin_artifacts_replace_html_and_refresh_downloads(self) -> None:
        self.write(self.site / "napkin" / "old.html", "old")
        self.add_site_artifact("dist-napkin-html", "new html")
        self.write(
            self.artifacts / "dist-napkin-pdf" / "Napkin-ko.pdf", "new pdf"
        )
        self.write(
            self.artifacts / "dist-napkin-epub" / "Napkin-ko.epub", "new epub"
        )
        self.add_devguide()

        result = self.run_stage()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.site / "napkin" / "old.html").exists())
        self.assertEqual(
            (self.site / "napkin" / "index.html").read_text(encoding="utf-8"),
            "new html",
        )
        self.assertEqual(
            (self.site / "napkin" / "Napkin-ko.pdf").read_text(encoding="utf-8"),
            "new pdf",
        )
        self.assertEqual(
            (self.site / "napkin" / "Napkin-ko.epub").read_text(encoding="utf-8"),
            "new epub",
        )

    def test_required_devguide_and_landing_files_are_refreshed(self) -> None:
        self.write(self.site / "devguide" / "old.html", "old devguide")
        self.write(self.site / "favicon.svg", "old favicon")
        self.add_devguide()

        result = self.run_stage()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.site / "devguide" / "old.html").exists())
        self.assertEqual(
            (self.site / "devguide" / "index.html").read_text(encoding="utf-8"),
            "devguide",
        )
        self.assertEqual(
            (self.site / "index.html").read_text(encoding="utf-8"), "new root"
        )
        self.assertEqual(
            (self.site / "favicon.svg").read_text(encoding="utf-8"),
            "new favicon",
        )


if __name__ == "__main__":
    unittest.main()
