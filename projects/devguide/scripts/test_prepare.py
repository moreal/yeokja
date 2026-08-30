import tempfile
import unittest
from pathlib import Path

from projects.devguide.scripts.prepare import MANAGED_BLOCK, prepare


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
