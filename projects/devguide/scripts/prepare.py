import os
import re
import stat
import sys
import tempfile
from pathlib import Path


BEGIN_MARKER = "# BEGIN YEOKJA KOREAN CONFIG"
END_MARKER = "# END YEOKJA KOREAN CONFIG"
MANAGED_BLOCK = """# BEGIN YEOKJA KOREAN CONFIG
exclude_patterns = [*globals().get("exclude_patterns", []), "include/activate-tab.rst", "include/links.rst"]
language = "ko"
html_title = "Python 개발자 가이드 (비공식 한국어 번역)"
nitpick_ignore = [*globals().get("nitpick_ignore", []), ("rst:role", "py:func")]
# END YEOKJA KOREAN CONFIG"""

PROJECT_ANCHOR = 'project = "Python Developer\'s Guide"'
HTML_TITLE_ANCHOR = 'html_title = ""'
LANGUAGE_SETTING = re.compile(r"^\s*language\s*=", re.MULTILINE)


def _atomic_write(path: Path, text: str, mode: int) -> None:
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as temporary:
            temporary_path = Path(temporary.name)
            temporary.write(text)
            temporary.flush()
            os.fsync(temporary.fileno())
        os.chmod(temporary_path, mode)
        os.replace(temporary_path, path)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def prepare(conf_path: Path) -> None:
    text = conf_path.read_text(encoding="utf-8")
    begin_count = text.count(BEGIN_MARKER)
    end_count = text.count(END_MARKER)

    if begin_count == 1 and end_count == 1:
        return
    if begin_count or end_count:
        raise ValueError("invalid managed block markers")
    if LANGUAGE_SETTING.search(text):
        raise ValueError("unmanaged language setting")
    if PROJECT_ANCHOR not in text:
        raise ValueError(f"missing pinned upstream anchor: {PROJECT_ANCHOR}")
    if HTML_TITLE_ANCHOR not in text:
        raise ValueError(f"missing pinned upstream anchor: {HTML_TITLE_ANCHOR}")

    separator = "\n" if text.endswith("\n") else "\n\n"
    mode = stat.S_IMODE(conf_path.stat().st_mode)
    _atomic_write(conf_path, text + separator + MANAGED_BLOCK + "\n", mode)


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1:
        print("usage: prepare.py <conf.py>", file=sys.stderr)
        return 2
    try:
        prepare(Path(args[0]))
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
