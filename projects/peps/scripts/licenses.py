#!/usr/bin/env python3
"""Audit per-PEP licenses and prepare safe publication placeholders."""

from __future__ import annotations

import argparse
from email.parser import Parser
import json
from dataclasses import dataclass
from pathlib import Path
import re
import subprocess
import sys
import tomllib


PROJECT_ROOT = Path(__file__).resolve().parent.parent
SOURCE_ROOT = PROJECT_ROOT / "upstream" / "peps"
MODIFIER = "moreal/yeokja 프로젝트"
MODIFIED_ON = "2026-08-29"
PEP_NAME = re.compile(r"pep-\d{4}\.rst\Z")
ADORNMENTS = set("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~")
LICENSE_HEADINGS = {
    "copyright",
    "copyright and license",
    "copyright/license",
    "copyright and/or license",
    "license",
}


@dataclass(frozen=True)
class LicenseRecord:
    name: str
    kind: str
    detail: str


def is_adornment(line: str) -> bool:
    stripped = line.strip()
    return (
        len(stripped) >= 3
        and stripped[0] in ADORNMENTS
        and all(character == stripped[0] for character in stripped)
    )


def license_section(text: str) -> str:
    """Return the final copyright/license section, or a small tail fallback."""
    lines = text.splitlines()
    start = None
    underline = None
    for index in range(len(lines) - 1):
        if lines[index].strip().casefold() in LICENSE_HEADINGS and is_adornment(lines[index + 1]):
            start = index
            underline = lines[index + 1].strip()[0]
    if start is None:
        return "\n".join(lines[-80:])

    end = len(lines)
    for index in range(start + 2, len(lines) - 1):
        if is_adornment(lines[index + 1]) and lines[index + 1].strip()[0] == underline:
            end = index
            break
    return "\n".join(lines[start:end])


def classify(path: Path) -> LicenseRecord:
    text = path.read_text(encoding="utf-8")
    section = license_section(text)
    folded = " ".join(section.casefold().split())

    if "don't even think about quoting, copying, modifying, or distributing" in folded:
        return LicenseRecord(path.name, "restricted", "copying, modification, and distribution prohibited")
    if "open publication license" in folded:
        return LicenseRecord(path.name, "open-publication-license", "Open Publication License 1.0 or later")
    if "public domain" in folded or "cc0" in folded:
        detail = "Public Domain"
        if "cc0" in folded:
            detail = "Public Domain or CC0-1.0, whichever is more permissive"
        return LicenseRecord(path.name, "public-domain-or-cc0", detail)
    return LicenseRecord(path.name, "unverified", "no explicit reusable license found")


def records() -> list[LicenseRecord]:
    return [classify(path) for path in sorted(SOURCE_ROOT.glob("pep-*.rst"))]


def excluded_names(all_records: list[LicenseRecord]) -> list[str]:
    return [record.name for record in all_records if record.kind in {"restricted", "unverified"}]


def configured_exclusions() -> list[str]:
    config = tomllib.loads((PROJECT_ROOT / "yeokja.toml").read_text(encoding="utf-8"))
    exclusions: list[str] = []
    for source in config.get("sources", []):
        if source.get("path") == "upstream/peps" and source.get("pattern") == "pep-*.rst":
            exclusions.extend(source.get("exclude", []))
    return sorted(exclusions)


def audit(check_config: bool) -> list[LicenseRecord]:
    all_records = records()
    by_kind: dict[str, list[LicenseRecord]] = {}
    for record in all_records:
        by_kind.setdefault(record.kind, []).append(record)

    print(f"Audited {len(all_records)} PEP documents:")
    for kind in sorted(by_kind):
        print(f"  {kind}: {len(by_kind[kind])}")
    for kind in ("restricted", "unverified"):
        for record in by_kind.get(kind, []):
            print(f"  exclude {record.name}: {record.detail}")

    if check_config:
        expected = excluded_names(all_records)
        configured = configured_exclusions()
        if configured != expected:
            print("yeokja.toml license exclusions do not match the audit", file=sys.stderr)
            print(f"expected: {expected}", file=sys.stderr)
            print(f"configured: {configured}", file=sys.stderr)
            raise SystemExit(1)
    return all_records


def placeholder(record: LicenseRecord, source: str) -> str:
    original_url = f"https://peps.python.org/{record.name[:-4]}/"
    metadata = Parser().parsestr(source, headersonly=True)
    required = ("PEP", "Author", "Status", "Type", "Created")
    missing = [name for name in required if metadata[name] is None]
    if missing:
        raise ValueError(f"{record.name} is missing required metadata: {missing}")
    if record.kind == "restricted":
        reason = (
            "이 PEP은 원문에서 인용·복제·수정·배포를 허용하지 않는다고 명시하므로 "
            "한국어 번역본을 만들거나 이 사이트에 복제하지 않습니다."
        )
    else:
        reason = (
            "이 PEP에서는 재배포와 번역을 허용하는 라이선스를 확인하지 못했습니다. "
            "권리자의 명시적인 허락을 확인하기 전까지 한국어 번역본을 제공하지 않습니다."
        )
    # Rebuild only the factual fields required by the official PEP index. In
    # particular, do not copy the original title or any body prose from an
    # excluded document (PEP 401 explicitly prohibits copying).
    return f"""PEP: {metadata['PEP']}
Title: 한국어 번역 미제공 안내
Author: {metadata['Author']}
Status: {metadata['Status']}
Type: {metadata['Type']}
Created: {metadata['Created']}

번역을 제공하지 않는 PEP
==========================

{reason}

공식 원문: `Python.org에서 보기 <{original_url}>`__.
"""


def translated_notice(record: LicenseRecord, upstream_commit: str) -> str:
    original_url = f"https://peps.python.org/{record.name[:-4]}/"
    pinned_source_url = (
        f"https://github.com/python/peps/blob/{upstream_commit}/peps/{record.name}?plain=1"
    )
    if record.kind == "open-publication-license":
        body = (
            "이 문서는 Open Publication License v1.0 이상에 따라 만든 수정된 한국어 "
            f"번역본입니다. 수정자: {MODIFIER}. 수정일: {MODIFIED_ON}. 변경 내용: "
            "영어 원문을 한국어로 번역했습니다. 원저자와 저작권 표시는 위 Author 필드와 "
            "아래 Copyright 절에 유지했으며, 이 번역은 원저자의 승인이나 보증을 뜻하지 "
            f"않습니다. `수정되지 않은 기준 원문 <{pinned_source_url}>`__ · "
            f"`공식 최신판 <{original_url}>`__ · "
            "`Open Publication License v1.0 <https://spdx.org/licenses/OPUBL-1.0.html>`__"
        )
    else:
        body = (
            f"이 비공식 한국어 번역은 원문 Copyright 절의 **{record.detail}** 조건에 "
            f"따라 제공합니다. 원저자와 공식 원문은 그대로 표시합니다. `수정되지 않은 "
            f"기준 원문 <{pinned_source_url}>`__ · `공식 최신판 <{original_url}>`__"
        )
    return ".. admonition:: 번역·라이선스 안내\n   :class: translation-notice\n\n" + "\n".join(
        f"   {line}" for line in body.splitlines()
    )


def insert_after_preamble(source: str, notice: str) -> str:
    if "\n   :class: translation-notice\n" in source:
        return source
    match = re.search(r"\r?\n\r?\n", source)
    if match is None:
        raise ValueError("PEP has no terminating blank line after its metadata preamble")
    return source[: match.end()] + notice + "\n\n" + source[match.end() :]


def materialize(path: Path) -> str:
    """Turn an assembled symlink into a disposable real file before editing."""
    content = path.read_text(encoding="utf-8")
    if path.is_symlink():
        path.unlink()
        path.write_text(content, encoding="utf-8")
    return content


def prepare(build_tree: Path) -> None:
    all_records = audit(check_config=True)
    upstream_commit = subprocess.run(
        [
            "git",
            "-c",
            "core.fsmonitor=false",
            "-C",
            str(PROJECT_ROOT / "upstream"),
            "rev-parse",
            "HEAD",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    destination = build_tree.resolve() / "peps"
    if not destination.is_dir():
        raise SystemExit(f"build tree has no PEP source directory: {destination}")

    manifest = []
    for record in all_records:
        manifest.append({"pep": record.name[4:8], "kind": record.kind, "detail": record.detail})
        output_path = destination / record.name
        if record.kind in {"restricted", "unverified"}:
            source = (SOURCE_ROOT / record.name).read_text(encoding="utf-8")
            materialize(output_path)
            output_path.write_text(placeholder(record, source), encoding="utf-8")
        else:
            translated = materialize(output_path)
            output_path.write_text(
                insert_after_preamble(
                    translated, translated_notice(record, upstream_commit)
                ),
                encoding="utf-8",
            )
    (build_tree / "license-manifest.json").write_text(
        json.dumps(
            {"upstream_commit": upstream_commit, "documents": manifest},
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    conf_path = destination / "conf.py"
    conf = materialize(conf_path)
    conf = conf.replace('project = "PEPs"', 'project = "PEP 한국어 번역"')
    conf = conf.replace(
        'html_baseurl = "https://peps.python.org"',
        'html_baseurl = "https://moreal.github.io/yeokja/peps"',
    )
    extension_line = 'extensions.append("pep_korean_index")'
    if extension_line not in conf:
        conf += f"\n{extension_line}\n"
    conf_path.write_text(conf, encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    audit_parser = subparsers.add_parser("audit")
    audit_parser.add_argument("--check-config", action="store_true")
    prepare_parser = subparsers.add_parser("prepare")
    prepare_parser.add_argument("build_tree", type=Path)
    args = parser.parse_args()

    if args.command == "audit":
        audit(args.check_config)
    else:
        prepare(args.build_tree)


if __name__ == "__main__":
    main()
