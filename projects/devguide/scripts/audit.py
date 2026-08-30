import json
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit


class _DocumentParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.anchors: set[str] = set()
        self.hrefs: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        del tag
        for name, value in attrs:
            if value is None:
                continue
            if name in {"id", "name"}:
                self.anchors.add(value)
            elif name == "href":
                self.hrefs.append(value)


def _add_error(
    errors: list[tuple[str, str, str]], path: Path, segment: str, message: str
) -> None:
    errors.append((path.as_posix(), segment, message))


def _state_path(source_path: Path, source_root: Path, state_root: Path) -> Path:
    relative = source_path.relative_to(source_root)
    return state_root / f"{relative.as_posix()}.yeokja.json"


def audit_translation(
    source_root: Path, state_root: Path, output_root: Path
) -> list[str]:
    """Return deterministic completeness diagnostics for a translation tree."""
    source_root = source_root.resolve()
    state_root = state_root.resolve()
    output_root = output_root.resolve()
    errors: list[tuple[str, str, str]] = []
    source_files = sorted(source_root.rglob("*.rst")) if source_root.exists() else []
    source_relatives = {path.relative_to(source_root) for path in source_files}

    for source_path in source_files:
        relative = source_path.relative_to(source_root)
        state_path = _state_path(source_path, source_root, state_root)
        output_path = output_root / relative
        state_relative = state_path.relative_to(state_root)

        if not state_path.is_file():
            _add_error(
                errors,
                relative,
                "",
                f"missing state: {state_relative.as_posix()}",
            )
        else:
            _audit_state_file(state_path, state_relative, errors)
        if not output_path.is_file():
            _add_error(errors, relative, "", f"missing output: {relative.as_posix()}")

    state_files = (
        sorted(state_root.rglob("*.rst.yeokja.json")) if state_root.exists() else []
    )
    for state_path in state_files:
        state_relative = state_path.relative_to(state_root)
        source_relative = Path(
            state_relative.as_posix()[: -len(".yeokja.json")]
        )
        if source_relative not in source_relatives:
            _add_error(
                errors,
                source_relative,
                "",
                f"orphan state: {state_relative.as_posix()}",
            )

    output_files = (
        sorted(output_root.rglob("*.rst")) if output_root.exists() else []
    )
    for output_path in output_files:
        relative = output_path.relative_to(output_root)
        if relative not in source_relatives:
            _add_error(errors, relative, "", f"orphan output: {relative.as_posix()}")

    return [message for _, _, message in sorted(errors)]


def _audit_state_file(
    state_path: Path,
    state_relative: Path,
    errors: list[tuple[str, str, str]],
) -> None:
    label = state_relative.as_posix()
    try:
        payload = json.loads(state_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        _add_error(errors, state_relative, "", f"invalid state: {label}")
        return

    if not isinstance(payload, dict):
        _add_error(errors, state_relative, "", f"invalid state: {label}")
        return
    if type(payload.get("version")) is not int or payload["version"] != 1:
        _add_error(errors, state_relative, "", f"invalid version: {label}")

    segments = payload.get("segments")
    if not isinstance(segments, list):
        _add_error(errors, state_relative, "", f"invalid segments: {label}")
        return

    for index, segment in enumerate(segments):
        if not isinstance(segment, dict):
            _add_error(
                errors,
                state_relative,
                f"{index:08d}",
                f"invalid segment: {label}: {index}",
            )
            continue
        segment_id = segment.get("id")
        if not isinstance(segment_id, str):
            segment_id = f"{index:08d}"
        translation = segment.get("translation")
        if not isinstance(translation, str) or not translation.strip():
            _add_error(
                errors,
                state_relative,
                segment_id,
                f"missing translation: {label}: {segment_id}",
            )
        issues = segment.get("issues")
        if not isinstance(issues, list):
            _add_error(
                errors,
                state_relative,
                segment_id,
                f"invalid issues: {label}: {segment_id}",
            )
        elif issues:
            _add_error(
                errors,
                state_relative,
                segment_id,
                f"unresolved issues: {label}: {segment_id}",
            )


def audit_html(site_root: Path) -> list[str]:
    """Return deterministic local-link and fragment diagnostics for HTML output."""
    site_root = site_root.resolve()
    documents: dict[Path, _DocumentParser] = {}
    document_paths: dict[Path, Path] = {}
    errors: list[tuple[str, str, str]] = []
    for path in sorted(site_root.rglob("*.html")) if site_root.exists() else []:
        relative = path.relative_to(site_root)
        try:
            resolved_path = path.resolve()
            resolved_path.relative_to(site_root)
        except (OSError, RuntimeError, ValueError):
            _add_error(
                errors,
                relative,
                "",
                f"escapes site root: {relative.as_posix()}",
            )
            continue
        parser = _DocumentParser()
        try:
            parser.feed(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError):
            _add_error(errors, relative, "", f"invalid html: {relative.as_posix()}")
            continue
        parser.close()
        documents[resolved_path] = parser
        document_paths[resolved_path] = path

    for referring_path, parser in documents.items():
        referring_relative = document_paths[referring_path].relative_to(site_root)
        for href in parser.hrefs:
            try:
                parsed = urlsplit(href)
            except ValueError:
                _add_error(
                    errors,
                    referring_relative,
                    href,
                    f"invalid URL: {referring_relative.as_posix()}: {href}",
                )
                continue
            if parsed.scheme or parsed.netloc:
                continue

            target = _resolve_target(site_root, referring_path, unquote(parsed.path))
            if target is None:
                _add_error(
                    errors,
                    referring_relative,
                    href,
                    f"escapes site root: {referring_relative.as_posix()}: {href}",
                )
                continue
            if target.is_dir():
                target = target / "index.html"
            if not target.is_file():
                _add_error(
                    errors,
                    referring_relative,
                    href,
                    f"missing target: {referring_relative.as_posix()}: {href}",
                )
                continue

            fragment = unquote(parsed.fragment)
            if fragment and target.suffix.lower() == ".html":
                target_document = documents.get(target.resolve())
                if target_document is None or fragment not in target_document.anchors:
                    _add_error(
                        errors,
                        referring_relative,
                        href,
                        f"missing fragment: {referring_relative.as_posix()}: {href}",
                    )

    return [message for _, _, message in sorted(errors)]


def _resolve_target(site_root: Path, referring_path: Path, path: str) -> Path | None:
    if not path:
        return referring_path
    candidate = (
        site_root / path.lstrip("/")
        if path.startswith("/")
        else referring_path.parent / path
    )
    try:
        target = candidate.resolve()
    except (OSError, RuntimeError):
        return None
    try:
        target.relative_to(site_root)
    except ValueError:
        return None
    return target


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1 or args[0] not in {"translation", "html"}:
        print("usage: audit.py {translation|html}", file=sys.stderr)
        return 2

    project_root = Path(__file__).resolve().parents[1]
    if args[0] == "translation":
        errors = audit_translation(
            project_root / "upstream",
            project_root / "state" / "upstream",
            project_root / "ko",
        )
    else:
        errors = audit_html(project_root / "dist" / "site")
    for error in errors:
        print(error, file=sys.stderr)
    return int(bool(errors))


if __name__ == "__main__":
    raise SystemExit(main())
