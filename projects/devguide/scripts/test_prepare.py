import importlib
import io
import os
import runpy
import stat
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from projects.devguide.scripts.prepare import MANAGED_BLOCK, main, prepare


prepare_module = importlib.import_module("projects.devguide.scripts.prepare")


class PartialWriter:
    def __init__(self, handle):
        self.handle = handle

    @property
    def name(self):
        return self.handle.name

    def __enter__(self):
        self.handle.__enter__()
        return self

    def __exit__(self, *args):
        return self.handle.__exit__(*args)

    def write(self, text: str) -> None:
        self.handle.write(text[:8])
        self.handle.flush()
        raise OSError("disk full")


class PrepareTests(unittest.TestCase):
    def write_conf(self, root: str, text: str) -> Path:
        path = Path(root, "conf.py")
        path.write_text(text, encoding="utf-8")
        return path

    def test_appends_korean_settings_and_exact_nitpick(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            prepare(path)
            text = path.read_text(encoding="utf-8")
            self.assertEqual(text.count(MANAGED_BLOCK), 1)
            self.assertIn('language = "ko"', text)
            self.assertIn('("rst:role", "py:func")', text)

    def test_appends_include_only_fragments_without_dropping_exclusions(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                "\n".join(
                    [
                        'project = "Python Developer\'s Guide"',
                        'html_title = ""',
                        'exclude_patterns = ["_build", "README.rst"]',
                        "",
                    ]
                ),
            )

            prepare(path)
            config = runpy.run_path(str(path))

            self.assertEqual(
                config["exclude_patterns"],
                [
                    "_build",
                    "README.rst",
                    "include/activate-tab.rst",
                    "include/links.rst",
                ],
            )
            self.assertNotIn("suppress_warnings", config)

    def test_second_run_is_idempotent(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            prepare(path)
            first = path.read_bytes()
            prepare(path)
            self.assertEqual(path.read_bytes(), first)

    def test_existing_unmanaged_language_setting_fails(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\nlanguage = "ja"\n',
            )
            with self.assertRaisesRegex(ValueError, "unmanaged language"):
                prepare(path)

    def test_missing_conf_fails(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(FileNotFoundError):
                prepare(Path(root, "conf.py"))

    def test_partial_write_keeps_original_and_removes_temporary_file(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            original = path.read_bytes()
            named_temporary_file = tempfile.NamedTemporaryFile

            def failing_named_temporary_file(*args, **kwargs):
                return PartialWriter(named_temporary_file(*args, **kwargs))

            with mock.patch.object(
                prepare_module.tempfile,
                "NamedTemporaryFile",
                side_effect=failing_named_temporary_file,
            ):
                with self.assertRaisesRegex(OSError, "disk full"):
                    prepare(path)

            self.assertEqual(path.read_bytes(), original)
            self.assertEqual(list(Path(root).iterdir()), [path])

    def test_replace_failure_keeps_original_and_removes_temporary_file(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            original = path.read_bytes()

            with mock.patch.object(
                prepare_module.os,
                "replace",
                side_effect=OSError("replace failed"),
            ):
                with self.assertRaisesRegex(OSError, "replace failed"):
                    prepare(path)

            self.assertEqual(path.read_bytes(), original)
            self.assertEqual(list(Path(root).iterdir()), [path])

    def test_atomic_replace_preserves_original_mode(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            os.chmod(path, 0o640)

            prepare(path)

            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o640)

    def test_cli_prints_write_error_and_exits_nonzero(self):
        with tempfile.TemporaryDirectory() as root:
            path = self.write_conf(
                root,
                'project = "Python Developer\'s Guide"\nhtml_title = ""\n',
            )
            stderr = io.StringIO()

            with mock.patch.object(
                prepare_module, "prepare", side_effect=OSError("disk full")
            ):
                with mock.patch("sys.stderr", stderr):
                    try:
                        result = main([str(path)])
                    except OSError as error:
                        self.fail(f"main propagated OSError: {error}")

            self.assertEqual(result, 1)
            self.assertEqual(stderr.getvalue(), "disk full\n")
