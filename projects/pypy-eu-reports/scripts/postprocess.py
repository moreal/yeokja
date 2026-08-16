#!/usr/bin/env python3
"""pdf2md(firecrawl/pdf-inspector) 출력의 알려진 아티팩트를 정정합니다.

원칙: 전면 재변환 대신 최소한의 재현 가능한 정정만 합니다. 모든 정정은
적용 횟수를 단언하므로, pdf-inspector 출력이 달라지면 조용히 어긋나는
대신 여기서 실패합니다. 원문의 오탈자(recieved, metaprograming, resolves
around 등)와 <!-- Page N --> 경계는 그대로 둡니다. 7쪽의 스프린트 사진은
텍스트 추출에 포함되지 않아 이 소스에 없습니다.

정정 목록 (PDF 원본과 페이지 단위 대조로 확인):
 1. 4쪽·6쪽 도식(figure)의 텍스트 라벨이 유사 표·산문으로 추출됨
    → 번역 대상에서 빠지도록 코드 펜스로 감쌉니다 (라벨은 원문 유지).
 2. 줄바꿈 하이픈 분절("deliv- ered") 재결합. 합성어(long-term 등)는
    하이픈을 유지하는 예외 목록으로 처리합니다.
 3. 불릿이 "- ** "로 추출됨 → "- "로 정정.
 4. 7쪽·8쪽 각주가 본문 문단 중간에 끼어듦 → 별도 문단으로 분리하고
    각주 번호를 원본의 위첨자로 되돌립니다.
 5. " - " 대시가 "-"로 붙어버린 곳(원본 확인분만)을 복원.
 6. 1쪽 저자 목록이 heading+bold 두 블록으로 갈라짐 → 한 문단으로 병합.
 7. 12쪽 연락처 표가 한 줄 산문으로 평탄화됨 → 원본 표 구조로 재구성.
"""

import re
import sys


def replace_exact(text: str, old: str, new: str, count: int = 1) -> str:
    found = text.count(old)
    assert found == count, f"expected {count}x, found {found}x: {old[:60]!r}"
    return text.replace(old, new)


def main() -> None:
    text = open(sys.argv[1], encoding="utf-8").read()

    # 6. 1쪽 저자 목록 병합 (heading/bold 분절 → 원본처럼 한 블록).
    text = replace_exact(
        text,
        "### Authors and Contributors: Holger Krekel, Lene Wagner, Jacob Hallén, Beatrice\n\n"
        "**During, Carl Friedrich Bolz, Laura Creighton, Armin Rigo, Michael Hudson, "
        "Samuele Pedroni, Christian Tismer, Alexandre Fayolle, Maciej Fijalkowski**",
        "**Authors and Contributors: Holger Krekel, Lene Wagner, Jacob Hallén, Beatrice "
        "During, Carl Friedrich Bolz, Laura Creighton, Armin Rigo, Michael Hudson, "
        "Samuele Pedroni, Christian Tismer, Alexandre Fayolle, Maciej Fijalkowski**",
    )

    # 4. 각주 1 (7쪽): 문단 끝에 끼어든 각주를 분리합니다. "participa-"는
    # 페이지 경계에서 갈라진 단어라 그대로 둡니다 (8쪽 첫 줄 "tion,"과 대응).
    text = replace_exact(
        text,
        " participa- 1 as we write this, the number of automated tests is 11805.",
        " participa-\n\n¹ as we write this, the number of automated tests is 11805.",
    )

    # 4. 각주 2 (8쪽): 위첨자 마커가 다음 줄 첫 단어 앞으로 밀려남.
    text = replace_exact(
        text,
        "as implemented in 2 PyPy and published their findings.",
        "as implemented in PyPy and published their findings.²",
    )
    text = replace_exact(
        text,
        "full share in the consortium duties 2The socGDS team",
        "full share in the consortium duties\n\n² The socGDS team",
    )
    # 각주 2의 끝 "2007."이 고아 문단으로 떨어져 나감 → 각주에 다시 붙입니다.
    text = replace_exact(
        text,
        "Limerick, Ireland,\n\n2007.",
        "Limerick, Ireland, 2007.",
    )

    # 5. 원본에서 " - "였던 대시가 붙어버린 곳 (PDF 3·4·6·7·8쪽 확인분).
    for old, new in [
        ("this claim-at least", "this claim - at least"),
        ("concerned-showing", "concerned - showing"),
        ("application development-as well as", "application development - as well as"),
        ("formal factors-on the one hand", "formal factors - on the one hand"),
        ("project context-an area", "project context - an area"),
        ("*sprints*- week long", "*sprints* - week long"),
    ]:
        text = replace_exact(text, old, new)

    # 5. 절 제목의 "PyPy - "도 같은 식으로 붙어버림 (목차 + 본문, 각 2회).
    for old, new in [
        ("PyPy-Vision and Relation", "PyPy - Vision and Relation"),
        ("PyPy-The Translation Framework", "PyPy - The Translation Framework"),
        ("PyPy-the flexible Python Runtime", "PyPy - the flexible Python Runtime"),
        ("PyPy-Sprint-Driven Development", "PyPy - Sprint-Driven Development"),
    ]:
        text = replace_exact(text, old, new, count=2)

    # 2. 하이픈 합성어는 재결합 시 하이픈을 유지합니다. "post-graduate"는
    # 줄바꿈 하이픈이라 원표기 판별이 불가능해, 같은 문장의 undergraduate와
    # 맞춰 붙여 씁니다. CALI- BRE는 뒷부분이 대문자라 일반 규칙 밖입니다.
    for old, new in [
        ("long- term", "long-term"),
        ("time- critical", "time-critical"),
        ("EU- projects", "EU-projects"),
        ("post- graduate", "postgraduate"),
        ("CALI- BRE", "CALIBRE"),
    ]:
        text = replace_exact(text, old, new)

    # 7. 12쪽 연락처 표 재구성 (원본 PDF의 2열 표 그대로).
    text = replace_exact(
        text,
        "#### Project Contact Data\n\n"
        "Project Web site [http://pypy.org](http://pypy.org) "
        "Press Contact Beatrice Düring <bea@changemaker.nu>, "
        "Holger Krekel <office@merlinux.de> "
        "Consortium Co-ordinator Stephan Busemann <busemann@dfki.de> "
        "Consortium Project Management pypy-manage@codespeak.net "
        "Consortium pypy-funding@codespeak.net "
        "Developer/Contributor core group pypy-ct@codespeak.net",
        "| Project Contact Data | |\n"
        "|---|---|\n"
        "| Project Web site | [http://pypy.org](http://pypy.org) |\n"
        "| Press Contact | Beatrice Düring <bea@changemaker.nu>, Holger Krekel <office@merlinux.de> |\n"
        "| Consortium Co-ordinator | Stephan Busemann <busemann@dfki.de> |\n"
        "| Consortium Project Management | pypy-manage@codespeak.net |\n"
        "| Consortium | pypy-funding@codespeak.net |\n"
        "| Developer/Contributor core group | pypy-ct@codespeak.net |",
    )

    # 1. 도식 영역을 코드 펜스로 감쌉니다. 시작/끝 줄(경계 라벨)로 찾습니다.
    lines = text.split("\n")
    figures = [
        ("|Python|Prolog|Scheme|", "prolog.net js-jvm prolog-js prolog-c pypy-llvm"),
        ("VM bytecode evaluator VM bytecode evaluator", "<function 'f'> <function 'f'>"),
    ]
    for start, end in figures:
        starts = [i for i, l in enumerate(lines) if l == start]
        ends = [i for i, l in enumerate(lines) if l == end]
        assert len(starts) == 1 and len(ends) == 1 and starts[0] < ends[0], (start, end)
        lines.insert(ends[0] + 1, "```")
        lines.insert(starts[0], "```text")

    # 2·3. 나머지 줄 단위 정정 — 펜스 안(도식 라벨)은 건드리지 않습니다.
    dehyphenated = 0
    bullets = 0
    in_fence = False
    for i, line in enumerate(lines):
        if line.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        line, n = re.subn(r"([A-Za-z])- ([a-z])", r"\1\2", line)
        dehyphenated += n
        if line.startswith("- ** "):
            line = "- " + line[len("- ** "):]
            bullets += 1
        lines[i] = line
    assert dehyphenated == 27, f"dehyphenated {dehyphenated}, expected 27"
    assert bullets == 17, f"bullets {bullets}, expected 17"

    sys.stdout.write("\n".join(lines))


if __name__ == "__main__":
    main()
