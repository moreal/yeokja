import json
import tempfile
import unittest
from pathlib import Path

from projects.devguide.scripts.audit import audit_html, audit_translation


class TranslationAuditTests(unittest.TestCase):
    def make_tree(self, root: str) -> tuple[Path, Path, Path]:
        base = Path(root)
        source = base / "upstream"
        state = base / "state" / "upstream"
        output = base / "ko"
        for path in (source, state, output):
            path.mkdir(parents=True)
        return source, state, output

    def write_complete(self, source: Path, state: Path, output: Path) -> None:
        (source / "index.rst").write_text("Hello.\n", encoding="utf-8")
        (output / "index.rst").write_text("안녕하세요.\n", encoding="utf-8")
        payload = {
            "version": 1,
            "source_hash": 1,
            "segments": [{
                "id": "section:0/block:0/seg:0",
                "source": "Hello.",
                "source_hash": 1,
                "context_hash": 1,
                "translation": "안녕하세요.",
                "glossary_snapshot": {},
                "translated_at": "2026-08-30T00:00:00Z",
                "issues": [],
            }],
        }
        (state / "index.rst.yeokja.json").write_text(
            json.dumps(payload), encoding="utf-8"
        )

    def test_complete_tree_has_no_errors(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            self.assertEqual(audit_translation(source, state, output), [])

    def test_reports_missing_state_and_output(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            (source / "index.rst").write_text("Hello.\n", encoding="utf-8")
            errors = audit_translation(source, state, output)
            self.assertIn("missing state: index.rst.yeokja.json", errors)
            self.assertIn("missing output: index.rst", errors)

    def test_reports_null_translation_and_issues(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            payload = json.loads(path.read_text(encoding="utf-8"))
            payload["segments"][0]["translation"] = None
            payload["segments"][0]["issues"] = [{"kind": "format"}]
            path.write_text(json.dumps(payload), encoding="utf-8")
            errors = audit_translation(source, state, output)
            self.assertTrue(any("missing translation" in error for error in errors))
            self.assertTrue(any("unresolved issues" in error for error in errors))

    def test_requires_complete_authoritative_segment_schema(self):
        required = {
            "id",
            "source",
            "source_hash",
            "context_hash",
            "translation",
            "glossary_snapshot",
            "translated_at",
            "issues",
        }
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            original = json.loads(path.read_text(encoding="utf-8"))

            for field in sorted(required):
                with self.subTest(field=field):
                    payload = json.loads(json.dumps(original))
                    del payload["segments"][0][field]
                    path.write_text(json.dumps(payload), encoding="utf-8")
                    diagnostic_id = (
                        "00000000"
                        if field == "id"
                        else "section:0/block:0/seg:0"
                    )
                    self.assertEqual(
                        audit_translation(source, state, output),
                        [
                            "invalid segment schema: index.rst.yeokja.json: "
                            + diagnostic_id
                        ],
                    )

            payload = json.loads(json.dumps(original))
            payload["segments"][0]["unexpected"] = "value"
            path.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(
                audit_translation(source, state, output),
                [
                    "invalid segment schema: index.rst.yeokja.json: "
                    "section:0/block:0/seg:0"
                ],
            )

    def test_rejects_invalid_segment_field_types_and_boolean_hashes(self):
        cases = [
            ("id", 1, "invalid id"),
            ("source", None, "invalid source"),
            ("source_hash", True, "invalid source_hash"),
            ("context_hash", True, "invalid context_hash"),
            ("translation", 1, "missing translation"),
            ("glossary_snapshot", [], "invalid glossary_snapshot"),
            ("translated_at", None, "invalid translated_at"),
            ("issues", {}, "invalid issues"),
        ]
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            original = json.loads(path.read_text(encoding="utf-8"))

            for field, value, message in cases:
                with self.subTest(field=field):
                    payload = json.loads(json.dumps(original))
                    payload["segments"][0][field] = value
                    path.write_text(json.dumps(payload), encoding="utf-8")
                    errors = audit_translation(source, state, output)
                    self.assertTrue(
                        any(message in error for error in errors),
                        (field, errors),
                    )

    def test_rejects_duplicate_segment_ids(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            payload = json.loads(path.read_text(encoding="utf-8"))
            payload["segments"].append(dict(payload["segments"][0]))
            path.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(
                audit_translation(source, state, output),
                [
                    "duplicate segment id: index.rst.yeokja.json: "
                    "section:0/block:0/seg:0"
                ],
            )

    def test_reports_orphan_state(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            (state / "orphan.rst.yeokja.json").write_text(
                '{"version": 1, "source_hash": 1, "segments": []}',
                encoding="utf-8",
            )
            self.assertIn(
                "orphan state: orphan.rst.yeokja.json",
                audit_translation(source, state, output),
            )

    def test_accepts_zero_segments_and_reports_invalid_state_fields(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            (source / "nested" / "raw.rst").parent.mkdir()
            (source / "nested" / "raw.rst").write_text(".. raw:: html\n", encoding="utf-8")
            (output / "nested").mkdir()
            (output / "nested" / "raw.rst").write_text(".. raw:: html\n", encoding="utf-8")
            path = state / "nested" / "raw.rst.yeokja.json"
            path.parent.mkdir()
            path.write_text('{"version": 1, "segments": []}', encoding="utf-8")
            self.assertEqual(audit_translation(source, state, output), [])
            path.write_text('{"version": 2, "segments": {}}', encoding="utf-8")
            errors = audit_translation(source, state, output)
            self.assertIn("invalid version: nested/raw.rst.yeokja.json", errors)
            self.assertIn("invalid segments: nested/raw.rst.yeokja.json", errors)

    def test_rejects_boolean_version(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            payload = json.loads(path.read_text(encoding="utf-8"))
            payload["version"] = True
            path.write_text(json.dumps(payload), encoding="utf-8")
            self.assertIn(
                "invalid version: index.rst.yeokja.json",
                audit_translation(source, state, output),
            )

    def test_reports_invalid_utf8_state(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            self.write_complete(source, state, output)
            path = state / "index.rst.yeokja.json"
            path.write_bytes(b"\xff")
            errors = audit_translation(source, state, output)
            self.assertEqual(errors, sorted(errors))
            self.assertTrue(any("invalid state: index.rst.yeokja.json" in error for error in errors))

    def test_enforces_reader_metadata_translation_and_protected_labels(self):
        with tempfile.TemporaryDirectory() as root:
            source, state, output = self.make_tree(root)
            (source / "index.rst").write_text(
                ".. topic:: Brett Cannon (Canada)\n\n"
                ".. tab:: Windows\n\n"
                ".. tab:: Other / pip\n",
                encoding="utf-8",
            )
            (output / "index.rst").write_text(
                ".. topic:: 브렛 캐넌 (Canada)\n\n"
                ".. tab:: 윈도우\n\n"
                ".. tab:: Other / pip\n",
                encoding="utf-8",
            )
            (state / "index.rst.yeokja.json").write_text(
                '{"version": 1, "segments": []}', encoding="utf-8"
            )

            errors = audit_translation(source, state, output)

            self.assertIn(
                "reader topic name changed: index.rst: Brett Cannon", errors
            )
            self.assertIn(
                "reader topic country not Korean: index.rst: Canada", errors
            )
            self.assertIn(
                "protected tab label changed: index.rst: Windows", errors
            )
            self.assertIn(
                "descriptive tab label not translated: index.rst: Other / pip",
                errors,
            )


class HtmlAuditTests(unittest.TestCase):
    def test_accepts_supported_local_and_external_links(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "guide").mkdir()
            (site / "_static").mkdir()
            (site / "_static" / "app.css").write_text("", encoding="utf-8")
            (site / "guide" / "index.html").write_text(
                '<h1 id="target">Guide</h1>', encoding="utf-8"
            )
            (site / "index.html").write_text(
                '<h1 id="intro">Index</h1>'
                '<a href="guide/">guide</a>'
                '<a href="guide/#target">target</a>'
                '<a href="#intro">intro</a>'
                '<a href="/_static/app.css">css</a>'
                '<a href="https://example.com/">external</a>'
                '<a href="mailto:docs@example.com">mail</a>',
                encoding="utf-8",
            )
            self.assertEqual(audit_html(site), [])

    def test_reports_missing_file_and_fragment(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "index.html").write_text(
                '<a href="missing.html">missing</a>'
                '<a href="#absent">fragment</a>',
                encoding="utf-8",
            )
            errors = audit_html(site)
            self.assertTrue(any("missing target" in error for error in errors))
            self.assertTrue(any("missing fragment" in error for error in errors))

    def test_decodes_named_anchors_and_rejects_site_root_escapes(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "target.html").write_text(
                '<a name="한글 앵커">target</a>', encoding="utf-8"
            )
            (site / "index.html").write_text(
                '<a href="target.html#%ED%95%9C%EA%B8%80%20%EC%95%B5%EC%BB%A4">target</a>'
                '<a href="../escape.html">escape</a>',
                encoding="utf-8",
            )
            errors = audit_html(site)
            self.assertEqual(len(errors), 1)
            self.assertIn("escapes site root", errors[0])

    def test_fragment_only_and_query_only_links_target_non_index_document(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "guide").mkdir()
            (site / "guide" / "topic.html").write_text(
                '<h1 id="section">Topic</h1>'
                '<a href="#section">fragment</a>'
                '<a href="?view=full">query</a>',
                encoding="utf-8",
            )
            self.assertEqual(audit_html(site), [])

    def test_reports_invalid_utf8_html(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "bad.html").write_bytes(b"\xff")
            errors = audit_html(site)
            self.assertEqual(errors, sorted(errors))
            self.assertTrue(any("invalid html: bad.html" in error for error in errors))

    def test_reports_malformed_url(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "index.html").write_text(
                '<a href="http://[broken">bad url</a>', encoding="utf-8"
            )
            errors = audit_html(site)
            self.assertEqual(errors, sorted(errors))
            self.assertTrue(any("invalid URL: index.html" in error for error in errors))

    def test_reports_percent_decoded_nul_path(self):
        with tempfile.TemporaryDirectory() as root:
            site = Path(root)
            (site / "index.html").write_text(
                '<a href="%00">nul</a>', encoding="utf-8"
            )
            self.assertEqual(
                audit_html(site),
                ["escapes site root: index.html: %00"],
            )

    def test_rejects_external_html_symlink_before_reading(self):
        with tempfile.TemporaryDirectory() as root:
            base = Path(root)
            site = base / "site"
            site.mkdir()
            outside = base / "outside.html"
            outside.write_text('<a href="missing.html">outside</a>', encoding="utf-8")
            linked = site / "linked.html"
            try:
                linked.symlink_to(outside)
            except (NotImplementedError, OSError) as error:
                self.skipTest(f"symlinks are unavailable: {error}")
            self.assertEqual(
                audit_html(site),
                ["escapes site root: linked.html"],
            )


if __name__ == "__main__":
    unittest.main()
