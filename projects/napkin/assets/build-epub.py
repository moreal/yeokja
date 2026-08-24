#!/usr/bin/env python3
"""Package the chaptered Korean Napkin HTML as a validated EPUB 3 book."""

from __future__ import annotations

import argparse
import base64
from html import escape, unescape
from html.parser import HTMLParser
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import unquote, urlsplit
import zipfile
import xml.etree.ElementTree as ET


OBJECT = re.compile(r"(?is)<object\b(?P<attrs>[^>]*)>\s*</object\s*>")
SVG = re.compile(r"(?is)<svg\b[^>]*>.*?</svg\s*>")
MATHML = re.compile(r"(?is)<math\b[^>]*>.*?</math\s*>")
MATHML_PLACEHOLDER = re.compile(
    r'''(?is)<span\b
        (?=[^>]*\bclass="[^"]*\bnapkin-math\b[^"]*")
        (?=[^>]*\bdata-mathml="(?P<mathml>[^"]+)")
        [^>]*>.*?</span\s*>''',
    re.VERBOSE,
)
SVG_PLACEHOLDER = re.compile(
    r'''(?is)<span\b
        (?=[^>]*\bclass="[^"]*\bnapkin-svg\b[^"]*")
        (?=[^>]*\bdata-svg="(?P<svg>[^"]+)")
        [^>]*>.*?</span\s*>''',
    re.VERBOSE,
)
HREF = re.compile(
    r'''(?is)(?P<prefix>\bhref\s*=\s*)(?P<quote>["'])
        (?P<value>.*?)(?P=quote)''',
    re.VERBOSE,
)
ATTRIBUTE = re.compile(
    r'''(?ix)(?P<name>[\w:-]+)\s*=\s*(?:"(?P<double>[^"]*)"|'(?P<single>[^']*)')'''
)
EPUB_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


class HeadMetadataParser(HTMLParser):
    """Read the title and next-page relation from a LaTeXML HTML head."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.in_title = False
        self.title_parts: list[str] = []
        self.next_href: str | None = None

    @property
    def title(self) -> str:
        return "".join(self.title_parts).strip()

    def handle_starttag(
        self, tag: str, attrs: list[tuple[str, str | None]]
    ) -> None:
        if tag == "title":
            self.in_title = True
            return
        if tag != "link":
            return
        attributes = dict(attrs)
        relations = (attributes.get("rel") or "").lower().split()
        if "next" in relations and attributes.get("href"):
            self.next_href = attributes["href"]

    def handle_endtag(self, tag: str) -> None:
        if tag == "title":
            self.in_title = False

    def handle_data(self, data: str) -> None:
        if self.in_title:
            self.title_parts.append(data)


def parse_head(path: Path) -> HeadMetadataParser:
    parser = HeadMetadataParser()
    parser.feed(path.read_text(encoding="utf-8"))
    return parser


def reading_order(site: Path) -> list[Path]:
    """Follow LaTeXML's rel=next chain, which is the book's canonical order."""

    root = site.resolve()
    current = root / "index.html"
    pages: list[Path] = []
    seen: set[Path] = set()

    while True:
        current = current.resolve()
        if current in seen:
            raise RuntimeError(f"cycle in HTML reading order at {current.name}")
        if current.parent != root or current.suffix.lower() != ".html":
            raise RuntimeError(f"reading order left the site directory: {current}")
        if not current.is_file():
            raise FileNotFoundError(f"reading-order page does not exist: {current}")

        seen.add(current)
        pages.append(current)
        metadata = parse_head(current)
        if metadata.next_href is None:
            break

        target = urlsplit(metadata.next_href)
        if target.scheme or target.netloc:
            raise RuntimeError(
                f"external rel=next target in {current.name}: {metadata.next_href}"
            )
        next_path = unquote(target.path)
        if not next_path:
            raise RuntimeError(
                f"rel=next has no local path in {current.name}: {metadata.next_href}"
            )
        current = current.parent / next_path

    return pages


def object_to_image(match: re.Match[str]) -> str:
    attributes = {
        item.group("name").lower(): unescape(
            item.group("double")
            if item.group("double") is not None
            else item.group("single") or ""
        )
        for item in ATTRIBUTE.finditer(match.group("attrs"))
    }
    source = attributes.get("data")
    if not source:
        raise RuntimeError("an HTML object has no data source")

    image_attributes = [("src", source)]
    for name in ("id", "class", "width", "height"):
        if name in attributes:
            image_attributes.append((name, attributes[name]))
    image_attributes.append(("alt", attributes.get("aria-label", "도표")))
    serialized = " ".join(
        f'{name}="{escape(value, quote=True)}"' for name, value in image_attributes
    )
    return f"<img {serialized}>"


def mathml_to_placeholder(match: re.Match[str]) -> str:
    encoded = base64.b64encode(match.group(0).encode("utf-8")).decode("ascii")
    return (
        f'<span class="napkin-math" data-mathml="{encoded}">'
        "[수식]</span>"
    )


def svg_to_placeholder(match: re.Match[str]) -> str:
    encoded = base64.b64encode(match.group(0).encode("utf-8")).decode("ascii")
    return f'<span class="napkin-svg" data-svg="{encoded}">[도표]</span>'


def prepare_inputs(pages: list[Path], destination: Path) -> list[Path]:
    destination.mkdir(parents=True)
    prepared: list[Path] = []
    for page in pages:
        html = page.read_text(encoding="utf-8")
        html, object_count = OBJECT.subn(object_to_image, html)
        # Protect inline SVG before MathML: commutative diagrams contain math
        # inside foreignObject elements and must survive as one XML subtree.
        html, svg_count = SVG.subn(svg_to_placeholder, html)
        html, mathml_count = MATHML.subn(mathml_to_placeholder, html)
        if "<object" in html.lower():
            raise RuntimeError(f"unsupported non-empty object element in {page.name}")
        output = destination / page.name
        output.write_text(html, encoding="utf-8")
        prepared.append(output)
        if object_count or svg_count or mathml_count:
            print(
                f"Prepared {page.name}: {object_count} diagrams, "
                f"{svg_count} inline SVGs, {mathml_count} MathML expressions"
            )
    return prepared


def add_namespace(markup: str, element: str, namespace: str) -> str:
    opening = re.search(fr"(?is)<{element}\b[^>]*>", markup)
    if opening is None or re.search(r"(?i)\bxmlns\s*=", opening.group(0)):
        return markup
    tag = opening.group(0)
    namespaced = tag[:-1] + f' xmlns="{namespace}">'
    return markup[: opening.start()] + namespaced + markup[opening.end() :]


def normalize_mathml(markup: str) -> str:
    # LaTeXML restarts short MathML IDs (for example, S2.m1) in each HTML
    # page. Pandoc copies every heading into one navigation document, so those
    # otherwise page-local IDs would collide there.
    markup = re.sub(
        r'''(?is)(<math\b[^>]*?)\s+id\s*=\s*(?:"[^"]*"|'[^']*')''',
        r"\1",
        markup,
        count=1,
    )
    markup = add_namespace(markup, "math", "http://www.w3.org/1998/Math/MathML")

    # Presentation MathML can contain human-readable HTML links inside mtext.
    # Their original page targets do not exist after Pandoc repaginates the
    # book, so retain the link text and make the mixed XHTML namespace explicit.
    markup = re.sub(r"(?is)<a\b[^>]*>", "<span>", markup)
    markup = re.sub(r"(?is)</a\s*>", "</span>", markup)
    return re.sub(
        r"(?is)<span\b(?P<attrs>[^>]*)>",
        lambda match: match.group(0)
        if re.search(r"(?i)\bxmlns\s*=", match.group("attrs"))
        else (
            f'<span{match.group("attrs")} '
            'xmlns="http://www.w3.org/1999/xhtml">'
        ),
        markup,
    )


def normalize_svg(markup: str) -> str:
    markup = add_namespace(markup, "svg", "http://www.w3.org/2000/svg")
    # HTML parsing treats children of foreignObject as XHTML integration
    # points. XML-based EPUB readers need that namespace stated explicitly.
    return re.sub(
        r'''(?is)(<foreignObject\b[^>]*>\s*)
            <(?P<tag>span|div)\b(?P<attrs>[^>]*)>''',
        lambda match: (
            match.group(1)
            + f'<{match.group("tag")}{match.group("attrs")} '
            + 'xmlns="http://www.w3.org/1999/xhtml">'
        )
        if "xmlns=" not in match.group("attrs").lower()
        else match.group(0),
        markup,
        flags=re.VERBOSE,
    )


def epub_page_targets(
    root: Path,
    pages: list[Path],
) -> dict[str, Path]:
    """Match source HTML filenames to Pandoc's spine content documents."""

    package = root / "EPUB/content.opf"
    tree = ET.parse(package)
    namespace = {"opf": "http://www.idpf.org/2007/opf"}
    package_directory = package.parent.relative_to(root)
    manifest: dict[str, Path] = {}
    for item in tree.findall(".//opf:manifest/opf:item", namespace):
        identifier = item.get("id")
        href = item.get("href")
        if identifier and href:
            manifest[identifier] = package_directory / unquote(urlsplit(href).path)

    content_documents: list[Path] = []
    for itemref in tree.findall(".//opf:spine/opf:itemref", namespace):
        resource = manifest.get(itemref.get("idref") or "")
        if resource and re.fullmatch(r"ch\d+\.xhtml", resource.name):
            content_documents.append(resource)
    if len(content_documents) != len(pages):
        raise RuntimeError(
            f"Pandoc produced {len(content_documents)} spine documents for "
            f"{len(pages)} source pages"
        )

    return dict(zip((page.name for page in pages), content_documents, strict=True))


def epub_target_identifiers(
    root: Path,
    targets: dict[str, Path],
) -> dict[str, set[str]]:
    identifiers: dict[str, set[str]] = {}
    for filename, resource in targets.items():
        text = (root / resource).read_text(encoding="utf-8")
        identifiers[filename] = {
            unescape(match.group("id"))
            for match in re.finditer(r'''\bid="(?P<id>[^"]+)"''', text)
        }
    return identifiers


def rewrite_internal_links(
    text: str,
    document: Path,
    root: Path,
    targets: dict[str, Path],
    identifiers: dict[str, set[str]],
) -> tuple[str, int]:
    """Rewrite LaTeXML page links to Pandoc's generated spine resources."""

    rewritten = 0

    def replace(match: re.Match[str]) -> str:
        nonlocal rewritten
        value = unescape(match.group("value"))
        target = urlsplit(value)
        if target.scheme or target.netloc:
            return match.group(0)
        filename = Path(unquote(target.path)).name
        fragment = unquote(target.fragment)
        if not filename and fragment:
            # With relative input names, Pandoc's file-scope pass represents
            # cross-input links as #chapter.html or #chapter.html__target.
            for source_name in targets:
                scoped_prefix = source_name.casefold() + "__"
                if fragment.casefold() == source_name.casefold():
                    filename = source_name
                    fragment = ""
                    break
                if fragment.casefold().startswith(scoped_prefix):
                    filename = source_name
                    break
        if not filename and target.fragment:
            current_resource = document.relative_to(root)
            filename = next(
                (
                    source_name
                    for source_name, resource in targets.items()
                    if resource == current_resource
                ),
                "",
            )
        target_resource = targets.get(filename)
        if target_resource is None:
            return match.group(0)

        resolved_fragment = ""
        if fragment:
            candidates = sorted(
                identifier
                for identifier in identifiers[filename]
                if identifier == fragment
                or identifier.endswith(f"__{fragment}")
                or fragment.endswith(f"__{identifier}")
            )
            if len(candidates) != 1:
                raise RuntimeError(
                    f"could not resolve {value!r} from {document.name}: "
                    f"found {len(candidates)} matching EPUB IDs"
                )
            resolved_fragment = "#" + candidates[0]

        absolute_target = root / target_resource
        relative_target = os.path.relpath(absolute_target, document.parent)
        new_value = Path(relative_target).as_posix()
        if target.query:
            new_value += "?" + target.query
        new_value += resolved_fragment
        rewritten += 1
        return (
            match.group("prefix")
            + match.group("quote")
            + escape(new_value, quote=True)
            + match.group("quote")
        )

    return HREF.sub(replace, text), rewritten


def restore_embedded_markup(epub: Path, pages: list[Path]) -> tuple[int, int, int]:
    """Restore protected MathML/SVG and mark their OPF manifest properties."""

    with tempfile.TemporaryDirectory(prefix="napkin-epub-restore-") as temporary:
        root = Path(temporary)
        with zipfile.ZipFile(epub) as archive:
            archive.extractall(root)

        targets = epub_page_targets(root, pages)
        math_documents: set[Path] = set()
        svg_documents: set[Path] = set()
        mathml_count = 0
        svg_count = 0
        link_count = 0
        for document in sorted(root.rglob("*.xhtml")):
            text = document.read_text(encoding="utf-8")

            # Restore outer MathML first. Some formulas contain an SVG
            # placeholder inside mtext; the following SVG pass then restores
            # that diagram and any MathML nested inside its foreignObjects.
            def restore_mathml(match: re.Match[str]) -> str:
                nonlocal mathml_count
                encoded = unescape(match.group("mathml"))
                mathml_count += 1
                return normalize_mathml(base64.b64decode(encoded).decode("utf-8"))

            text, restored_mathml = MATHML_PLACEHOLDER.subn(restore_mathml, text)

            def restore_svg(match: re.Match[str]) -> str:
                nonlocal mathml_count, svg_count
                encoded = unescape(match.group("svg"))
                markup = normalize_svg(base64.b64decode(encoded).decode("utf-8"))
                svg_count += 1
                mathml_count += len(MATHML.findall(markup))
                return re.sub(
                    MATHML,
                    lambda item: normalize_mathml(item.group(0)),
                    markup,
                )

            text, restored_svg = SVG_PLACEHOLDER.subn(restore_svg, text)
            if 'data-mathml="' in text or 'data-svg="' in text:
                raise RuntimeError(
                    f"protected MathML/SVG placeholder remains in {document.name}"
                )
            if restored_svg or restored_mathml:
                document.write_text(text, encoding="utf-8")
            relative_document = document.relative_to(root)
            if restored_mathml or (restored_svg and MATHML.search(text)):
                math_documents.add(relative_document)
            if restored_svg:
                svg_documents.add(relative_document)

        # Resolve links after every embedded SVG has been restored, because a
        # few same-page figure links target the SVG root itself.
        identifiers = epub_target_identifiers(root, targets)
        for document in sorted(root.rglob("*.xhtml")):
            text = document.read_text(encoding="utf-8")
            text, rewritten_links = rewrite_internal_links(
                text,
                document,
                root,
                targets,
                identifiers,
            )
            link_count += rewritten_links
            if rewritten_links:
                document.write_text(text, encoding="utf-8")

        if mathml_count == 0:
            raise RuntimeError("Pandoc output contains no protected MathML placeholders")

        package = root / "EPUB/content.opf"
        ET.register_namespace("", "http://www.idpf.org/2007/opf")
        ET.register_namespace("dc", "http://purl.org/dc/elements/1.1/")
        tree = ET.parse(package)
        opf_namespace = {"opf": "http://www.idpf.org/2007/opf"}
        package_directory = package.parent.relative_to(root)
        marked_math: set[Path] = set()
        marked_svg: set[Path] = set()
        for item in tree.findall(".//opf:manifest/opf:item", opf_namespace):
            href = item.get("href")
            if not href:
                continue
            resource = package_directory / unquote(urlsplit(href).path)
            properties = set((item.get("properties") or "").split())
            if resource in math_documents:
                properties.add("mathml")
                marked_math.add(resource)
            if resource in svg_documents:
                properties.add("svg")
                marked_svg.add(resource)
            if not properties:
                continue
            item.set("properties", " ".join(sorted(properties)))
        if marked_math != math_documents:
            missing = sorted(str(path) for path in math_documents - marked_math)
            raise RuntimeError(
                "MathML documents are absent from the OPF manifest: " + ", ".join(missing)
            )
        if marked_svg != svg_documents:
            missing = sorted(str(path) for path in svg_documents - marked_svg)
            raise RuntimeError(
                "SVG documents are absent from the OPF manifest: " + ", ".join(missing)
            )
        tree.write(package, encoding="utf-8", xml_declaration=True)

        def write_entry(
            archive: zipfile.ZipFile,
            path: Path,
            archive_name: str,
            compression: int,
        ) -> None:
            info = zipfile.ZipInfo(archive_name, date_time=EPUB_TIMESTAMP)
            info.compress_type = compression
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, path.read_bytes())

        rebuilt = epub.with_suffix(".repacked.epub")
        with zipfile.ZipFile(rebuilt, "w") as archive:
            write_entry(
                archive,
                root / "mimetype",
                "mimetype",
                zipfile.ZIP_STORED,
            )
            for path in sorted(root.rglob("*")):
                if not path.is_file() or path == root / "mimetype":
                    continue
                write_entry(
                    archive,
                    path,
                    path.relative_to(root).as_posix(),
                    zipfile.ZIP_DEFLATED,
                )
        rebuilt.replace(epub)
        return mathml_count, svg_count, link_count


def validate_archive(epub: Path) -> tuple[int, int, int]:
    with zipfile.ZipFile(epub) as archive:
        entries = archive.infolist()
        if not entries or entries[0].filename != "mimetype":
            raise RuntimeError("EPUB mimetype is not the first archive entry")
        if entries[0].compress_type != zipfile.ZIP_STORED:
            raise RuntimeError("EPUB mimetype must be stored without compression")
        if archive.read("mimetype") != b"application/epub+zip":
            raise RuntimeError("EPUB has an invalid mimetype")

        names = {entry.filename for entry in entries}
        required = {"META-INF/container.xml", "EPUB/content.opf", "EPUB/nav.xhtml"}
        missing = sorted(required - names)
        if missing:
            raise RuntimeError("EPUB is missing required entries: " + ", ".join(missing))

        documents = [name for name in names if name.endswith(".xhtml")]
        mathml_count = sum(
            archive.read(name).count(b"<math")
            for name in documents
        )
        svg_count = sum(
            archive.read(name).count(b"<svg")
            for name in documents
        )
        if mathml_count == 0:
            raise RuntimeError("EPUB contains no MathML despite the mathematical source")
        return len(documents), mathml_count, svg_count


def build_epub(
    site: Path,
    output: Path,
    css: Path,
    pandoc_filter: Path,
    pandoc: str,
) -> None:
    site = site.resolve()
    if not (site / "index.html").is_file():
        raise FileNotFoundError(f"HTML site has not been built: {site / 'index.html'}")
    css = css.resolve()
    pandoc_filter = pandoc_filter.resolve()
    cover = site / "cover-art.jpg"
    for required in (css, pandoc_filter, cover):
        if not required.is_file():
            raise FileNotFoundError(f"required EPUB asset is missing: {required}")

    pages = reading_order(site)
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="napkin-epub-") as temporary:
        inputs = prepare_inputs(pages, Path(temporary) / "html")
        command = [
            pandoc,
            "--from=html",
            "--to=epub3",
            "--file-scope",
            "--toc",
            "--toc-depth=2",
            "--split-level=1",
            f"--lua-filter={pandoc_filter}",
            "--epub-title-page=false",
            f"--resource-path={site}{os.pathsep}{site.parent}",
            f"--epub-cover-image={cover}",
            f"--css={css}",
            "--metadata=title:무한히 큰 냅킨",
            "--metadata=author:Evan Chen",
            "--metadata=lang:ko",
            "--metadata=identifier:https://moreal.github.io/yeokja/napkin/",
            "--metadata=source:https://github.com/vEnhance/napkin",
            "--metadata=rights:CC BY-SA 4.0; 비공식 한국어 번역본",
            f"--output={output}",
            *(path.name for path in inputs),
        ]
        completed = subprocess.run(
            command,
            cwd=inputs[0].parent,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if completed.stdout:
            print(completed.stdout, end="")
        if completed.stderr:
            print(completed.stderr, end="")
        completed.check_returncode()
        fatal_warnings = ("Could not fetch resource",)
        if any(warning in completed.stderr for warning in fatal_warnings):
            raise RuntimeError("Pandoc could not faithfully convert all EPUB content")

    restored_mathml, restored_svg, rewritten_links = restore_embedded_markup(
        output,
        pages,
    )
    documents, mathml, svg = validate_archive(output)
    # Pandoc's cover page may add its own inline SVG. The restored source
    # elements must all be present, but generated EPUB markup is allowed too.
    if mathml < restored_mathml or svg < restored_svg:
        raise RuntimeError(
            f"restored {restored_mathml} MathML/{restored_svg} SVG elements but "
            f"found {mathml} MathML/{svg} SVG elements in EPUB"
        )
    print(
        f"Built {output} from {len(pages)} pages "
        f"({documents} XHTML documents, {mathml} MathML expressions, "
        f"{svg} inline SVGs, {rewritten_links} rewritten links)"
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--site", type=Path, default=Path("site"))
    parser.add_argument("--output", type=Path, default=Path("Napkin-ko.epub"))
    parser.add_argument("--css", type=Path, default=Path("napkin-epub.css"))
    parser.add_argument(
        "--filter",
        type=Path,
        default=Path("napkin-epub.lua"),
        dest="pandoc_filter",
    )
    parser.add_argument("--pandoc", default="pandoc")
    args = parser.parse_args()

    if shutil.which(args.pandoc) is None:
        raise FileNotFoundError(
            f"{args.pandoc} is not installed; run this script through the Napkin Nix shell"
        )
    build_epub(
        args.site,
        args.output,
        args.css,
        args.pandoc_filter,
        args.pandoc,
    )


if __name__ == "__main__":
    main()
