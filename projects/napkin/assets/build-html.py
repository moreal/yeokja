#!/usr/bin/env python3
"""Build the Korean Napkin as a split, static HTML book with LaTeXML."""

from __future__ import annotations

import concurrent.futures
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


ASY_BLOCK = re.compile(
    r"(?ms)^[ \t]*\\begin\{asy\}(?:\[[^\n]*\])?(.*?)^[ \t]*\\end\{asy\}[ \t]*$"
)
ASY_DEFINITIONS = re.compile(
    r"(?ms)^[ \t]*\\begin\{asydef\}(.*?)^[ \t]*\\end\{asydef\}[ \t]*$"
)
TIKZ_BLOCK = re.compile(
    r"(?ms)\\begin\{tikzpicture\}(.*?)\\end\{tikzpicture\}"
)


def render_asymptote(task: tuple[Path, Path, Path, str]) -> None:
    root, source_path, output_path, source = task
    source_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    source_path.write_text(source, encoding="utf-8")
    subprocess.run(
        [
            "asy",
            "-quiet",
            "-f",
            "svg",
            "-tex",
            "latex",
            "-o",
            # Asymptote appends the selected format extension itself.
            str(output_path.with_suffix("").relative_to(root)),
            str(source_path.relative_to(root)),
        ],
        cwd=root,
        check=True,
    )


def replace_asymptote(root: Path) -> int:
    preamble = (root / "tex/preamble.tex").read_text(encoding="utf-8")
    match = ASY_DEFINITIONS.search(preamble)
    if match is None:
        raise RuntimeError("tex/preamble.tex has no asydef block")

    definitions = re.sub(
        r"(?m)^\s*settings\.(?:tex|outformat)\s*=.*?;\s*$", "", match.group(1)
    )
    tasks: list[tuple[Path, Path, Path, str]] = []

    for tex_path in sorted((root / "tex").rglob("*.tex")):
        text = tex_path.read_text(encoding="utf-8")
        relative = tex_path.relative_to(root).with_suffix("")
        index = 0

        def replacement(block: re.Match[str]) -> str:
            nonlocal index
            index += 1
            stem = "-".join(relative.parts) + f"-{index:02d}"
            asy_source = root / ".html-asy" / f"{stem}.asy"
            svg_output = root / "media/html-asy" / f"{stem}.svg"
            script = (
                definitions
                + "\nsettings.tex = \"latex\";\n"
                + "settings.outformat = \"svg\";\n"
                + block.group(1)
                + "\n"
            )
            tasks.append((root, asy_source, svg_output, script))
            return f"\\includegraphics{{media/html-asy/{stem}.svg}}"

        converted = ASY_BLOCK.sub(replacement, text)
        if index:
            tex_path.write_text(converted, encoding="utf-8")

    workers = min(4, max(1, os.cpu_count() or 1))
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        list(executor.map(render_asymptote, tasks))
    return len(tasks)


def render_latex_diagram(task: tuple[Path, str, str, bool]) -> None:
    root, stem, snippet, math_mode = task
    build_dir = root / ".html-diagram"
    output_path = root / "media/html-diagram" / f"{stem}.svg"
    build_dir.mkdir(parents=True, exist_ok=True)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    body = f"\\(\\displaystyle {snippet}\\)" if math_mode else snippet
    source = rf"""\documentclass[border=3pt]{{standalone}}
\usepackage{{fontspec}}
\setmainfont{{Noto Serif CJK KR}}
\setsansfont{{Noto Sans CJK KR}}
\usepackage{{amsmath,amssymb,amsthm,mathrsfs,mathtools,stmaryrd,wasysym}}
\usepackage[usenames,svgnames,dvipsnames]{{xcolor}}
\usepackage{{enumerate,multirow,graphicx,hyperref,tikz-cd}}
\input{{tex/macros}}
\input{{tex/Qcircuit.tex}}
\begin{{document}}
{body}
\end{{document}}
"""
    source_path = build_dir / f"{stem}.tex"
    source_path.write_text(source, encoding="utf-8")
    completed = subprocess.run(
        [
            "xelatex",
            "-interaction=nonstopmode",
            "-halt-on-error",
            f"-output-directory={build_dir}",
            str(source_path),
        ],
        cwd=root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if completed.returncode:
        print(completed.stdout)
        completed.check_returncode()
    subprocess.run(
        [
            "dvisvgm",
            "--pdf",
            "--no-fonts",
            "-o",
            str(output_path),
            str(build_dir / f"{stem}.pdf"),
        ],
        cwd=root,
        check=True,
        stdout=subprocess.DEVNULL,
    )


def balanced_group_end(source: str, opening: int) -> int:
    depth = 0
    for position in range(opening, len(source)):
        if source[position] not in "{}":
            continue
        backslashes = 0
        previous = position - 1
        while previous >= 0 and source[previous] == "\\":
            backslashes += 1
            previous -= 1
        if backslashes % 2:
            continue
        depth += 1 if source[position] == "{" else -1
        if depth == 0:
            return position + 1
    raise ValueError("unbalanced Qcircuit body")


def replace_latex_diagrams(root: Path) -> int:
    tasks: list[tuple[Path, str, str, bool]] = []
    for tex_path in sorted((root / "tex").rglob("*.tex")):
        if tex_path == root / "tex/Qcircuit.tex":
            continue
        text = tex_path.read_text(encoding="utf-8")
        original = text
        text = text.replace(
            r"\input{tex/frontmatter/digraph}", r"\input{Napkin-html-digraph}"
        )
        relative = "-".join(tex_path.relative_to(root).with_suffix("").parts)
        index = 0

        def replace_tikz(block: re.Match[str]) -> str:
            nonlocal index
            index += 1
            stem = f"{relative}-tikz-{index:02d}"
            snippet = "\\begin{tikzpicture}" + block.group(1) + "\\end{tikzpicture}"
            tasks.append((root, stem, snippet, False))
            return f"\\includegraphics{{media/html-diagram/{stem}.svg}}"

        text = TIKZ_BLOCK.sub(replace_tikz, text)

        search_from = 0
        replacements: list[tuple[int, int, str]] = []
        while True:
            start = text.find(r"\Qcircuit", search_from)
            if start == -1:
                break
            opening = text.find("{", start + len(r"\Qcircuit"))
            if opening == -1:
                raise ValueError(f"Qcircuit without a body in {tex_path}")
            end = balanced_group_end(text, opening)
            index += 1
            stem = f"{relative}-qcircuit-{index:02d}"
            tasks.append((root, stem, text[start:end], True))
            replacements.append(
                (start, end, f"\\includegraphics{{media/html-diagram/{stem}.svg}}")
            )
            search_from = end
        for start, end, replacement in reversed(replacements):
            text = text[:start] + replacement + text[end:]
        if text != original:
            tex_path.write_text(text, encoding="utf-8")

    workers = min(4, max(1, os.cpu_count() or 1))
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        list(executor.map(render_latex_diagram, tasks))
    return len(tasks)


def make_driver(root: Path) -> Path:
    source = (root / "Napkin.tex").read_text(encoding="utf-8")
    source = re.sub(
        r"^\\documentclass(?:\[[^\n]*\])?\{scrbook\}",
        r"\\documentclass[11pt]{book}",
        source,
        count=1,
        flags=re.MULTILINE,
    )
    source = source.replace(
        r"\input{tex/preamble}", r"\input{Napkin-html-preamble}"
    )
    source = source.replace("\\input{tex/macros}\n", "")
    source = source.replace("\\input{tex/Qcircuit.tex}\n", "")
    source = source.replace(
        r"\input{tex/frontmatter/digraph}", r"\input{Napkin-html-digraph}"
    )
    source = re.sub(
        r"(?m)^\\(?:Open|Close)solutionfile\{[^}]+\}\s*$", "", source
    )
    source = re.sub(
        r"(?m)^\\include\{tex/backmatter/hintsol\}\s*$", "", source
    )
    source = re.sub(
        r"(?m)^\\printbibliography(?:\[[^\n]*\])?\s*$", "", source
    )
    source = source.replace(
        r"\end{document}",
        "\\bibliographystyle{alpha}\n\\bibliography{references,images}\n\\end{document}",
    )
    driver = root / "Napkin-html.tex"
    driver.write_text(source, encoding="utf-8")
    return driver


def validate_pre_rendered_graphics(root: Path) -> None:
    references: set[str] = set()
    for tex_path in (root / "tex").rglob("*.tex"):
        text = tex_path.read_text(encoding="utf-8")
        references.update(
            re.findall(
                r"\\includegraphics\{((?:media/html-asy|media/html-diagram)/[^}]+)\}",
                text,
            )
        )
    missing = sorted(reference for reference in references if not (root / reference).is_file())
    if missing:
        raise FileNotFoundError(
            "missing pre-rendered graphics:\n" + "\n".join(missing)
        )


def add_site_chrome(site: Path) -> None:
    banner = """
<a class="napkin-skip" href="#napkin-main">본문으로 건너뛰기</a>
<header class="napkin-sitebar">
  <div class="napkin-sitebar-inner">
    <a class="napkin-brand" href="index.html">무한히 큰 냅킨</a>
    <nav aria-label="책 링크">
      <a href="index.html">목차</a>
      <a href="Napkin-ko.pdf" download>PDF 다운로드</a>
      <a href="Napkin-ko.epub" download>EPUB 다운로드</a>
    </nav>
  </div>
</header>
""".strip()
    mathjax = """
<script>
  window.MathJax = {
    svg: {fontCache: "global"},
    options: {enableMenu: true}
  };
</script>
<script defer src="mathjax/mml-svg.js"></script>
""".strip()

    for html_path in site.rglob("*.html"):
        html = html_path.read_text(encoding="utf-8")
        html = re.sub(r'(<html\b[^>]*?)\s+lang="[^"]*"', r"\1", html, count=1)
        html = html.replace("<html", '<html lang="ko"', 1)
        html = html.replace("</head>", f"{mathjax}\n</head>", 1)
        html = html.replace("<body>", f"<body>\n{banner}", 1)
        content = '<div class="ltx_page_content">'
        content_start = html.find(content)
        if content_start == -1:
            raise RuntimeError(f"LaTeXML page content missing in {html_path}")
        content_end = content_start + len(content)
        depth = 1
        closing_end = -1
        for tag in re.finditer(r"</?div\b[^>]*>", html[content_end:], re.I):
            depth += -1 if tag.group(0).startswith("</") else 1
            if depth == 0:
                closing_end = content_end + tag.end()
                break
        if closing_end == -1:
            raise RuntimeError(f"LaTeXML page content is unbalanced in {html_path}")
        html = html[:content_start] + '<main id="napkin-main">\n' + html[content_start:]
        closing_end += len('<main id="napkin-main">\n')
        html = html[:closing_end] + "\n</main>" + html[closing_end:]
        html_path.write_text(html, encoding="utf-8")


def main() -> None:
    source_root = Path.cwd()
    project_root = Path(os.environ.get("YEOKJA_ROOT", source_root)).resolve()

    with tempfile.TemporaryDirectory(prefix="napkin-html-") as temporary:
        work = Path(temporary) / "src"
        shutil.copytree(
            source_root,
            work,
            symlinks=False,
            ignore=shutil.ignore_patterns(
                "site", "dist", "output", "tmp", ".git", "__pycache__"
            ),
        )

        asymptote_count = replace_asymptote(work)
        latex_diagram_count = replace_latex_diagrams(work)
        validate_pre_rendered_graphics(work)
        driver = make_driver(work)
        site = work / "site"
        site.mkdir()
        log = work / "latexml.log"

        command = [
            "latexmlc",
            "--format=html5",
            f"--destination={site / 'index.html'}",
            f"--log={log}",
            "--timeout=2400",
            "--noparse",
            "--presentationmathml",
            "--split",
            "--splitat=chapter",
            "--splitnaming=label",
            "--navigationtoc=context",
            "--urlstyle=file",
            f"--css={work / 'napkin.css'}",
            "--svg",
            driver.name,
        ]
        try:
            subprocess.run(command, cwd=work, check=True)
        except subprocess.CalledProcessError:
            print(log.read_text(encoding="utf-8", errors="replace"))
            raise
        log_text = log.read_text(encoding="utf-8", errors="replace")
        errors = re.findall(r"(?m)^(?:Error|Fatal):.*$", log_text)
        if errors:
            print("\n".join(errors))
            raise RuntimeError(f"LaTeXML reported {len(errors)} conversion errors")

        pdf = project_root / "output/pdf/Napkin-ko.pdf"
        if not pdf.is_file():
            raise FileNotFoundError(
                f"{pdf} is missing; build or restore the committed Korean PDF first"
            )
        shutil.copy2(pdf, site / "Napkin-ko.pdf")
        mathjax_source = Path(os.environ.get("NAPKIN_MATHJAX_DIR", ""))
        if not mathjax_source.is_dir():
            raise FileNotFoundError(
                "NAPKIN_MATHJAX_DIR is missing; run the build through nix develop"
            )
        # Nixpkgs adds an `es5 -> .` compatibility link to MathJax 4. Copying
        # through that link would recurse forever, and modern MathJax does not
        # need the alias.
        shutil.copytree(
            mathjax_source,
            site / "mathjax",
            ignore=shutil.ignore_patterns("es5"),
        )
        # Nix store directories are read-only. copytree preserves those modes,
        # which would make the disposable build tree impossible to remove on
        # the next yeokja build even though its parent belongs to the user.
        for directory in [site / "mathjax", *(site / "mathjax").rglob("*")]:
            if directory.is_dir():
                directory.chmod(directory.stat().st_mode | 0o200)
        (site / ".nojekyll").touch()
        add_site_chrome(site)

        destination = source_root / "site"
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(site, destination)
        html_count = sum(1 for _ in destination.rglob("*.html"))
        print(
            f"Built {html_count} HTML pages with {asymptote_count} Asymptote "
            f"and {latex_diagram_count} TeX diagrams"
        )


if __name__ == "__main__":
    main()
