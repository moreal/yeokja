#!/bin/sh
# PYPY-EU-Final-Activity-Report.pdf을 받아 번역 소스 Markdown을 생성합니다.
#
# 요구 도구: curl, python3, firecrawl/pdf-inspector CLI
#   cargo install pdf-inspector   # detect-pdf, pdf2md 제공
#
# PDF와 생성물은 라이선스 미확정으로 커밋하지 않습니다(.gitignore) — README 참조.
set -eu
cd "$(dirname "$0")/.."

URL="https://foss.heptapod.net/pypy/extradoc/-/raw/branch/extradoc/eu-report/PYPY-EU-Final-Activity-Report.pdf"
PDF="source/PYPY-EU-Final-Activity-Report.pdf"
RAW="source/PYPY-EU-Final-Activity-Report.raw"
OUT="source/PYPY-EU-Final-Activity-Report.md"

mkdir -p source
[ -f "$PDF" ] || curl -fsSL -o "$PDF" "$URL"

# 분류 결과를 기록합니다. text_based / 12쪽 / OCR 필요 페이지 없음이어야 하며,
# 아니라면 postprocess.py의 전제가 깨진 것이므로 여기서 멈춥니다.
detect-pdf "$PDF" --analyze --json > source/detect.json
cat source/detect.json
grep -q '"pdf_type":"text_based"' source/detect.json
grep -q '"pages_needing_ocr":\[\]' source/detect.json

# <!-- Page N --> 경계를 보존해 이후 원본 PDF 대조에 사용합니다.
pdf2md "$PDF" --pages > "$RAW"

python3 scripts/postprocess.py "$RAW" > "$OUT"
echo "wrote $OUT"
