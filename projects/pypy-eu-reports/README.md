# PyPy EU Final Activity Report 한국어 번역

PyPy EU 프로젝트(IST FP6-004779, 2004-12 ~ 2007-03, 조정: DFKI Saarbrücken)의
최종 활동 보고서 *PyPy EU Final Activity Report* (2007-05-11, 12쪽)를
한국어로 번역하는 프로젝트입니다.

> 이 번역은 PyPy Project 또는 European Commission의 공식 한국어 번역이
> 아닙니다.

## 원본

- Canonical PDF: <https://foss.heptapod.net/pypy/extradoc/-/blob/branch/extradoc/eu-report/PYPY-EU-Final-Activity-Report.pdf>
- PyPy EU Reports 색인: <https://doc.pypy.org/en/latest/index-report.html>
- EU 프로젝트 정보(CORDIS): <https://cordis.europa.eu/project/id/004779>

## 라이선스 조사 결과 — 원문·번역물을 커밋하지 않는 이유

전체 복제·번역·재배포를 허용하는 근거를 확인하지 못했습니다:

- PDF 자체(12쪽 전체 확인)에 copyright/license notice가 없습니다. 본문의
  "MIT Open Source License" 언급(2장)은 소프트웨어(플랫폼·인터프리터·
  py.test)에 대한 것이지 보고서 텍스트에 대한 것이 아닙니다.
- `pypy/extradoc` 저장소에는 루트와 `eu-report/` 어디에도 LICENSE·README가
  없습니다 (저장소 트리로 확인, 2026-08-16).
- `pypy/pypy`의 [LICENSE](https://github.com/pypy/pypy/blob/main/LICENSE)(MIT)는
  그 저장소의 명시된 디렉터리만 대상으로 하며 extradoc은 포함하지 않습니다.
- CORDIS의 FP6-004779 페이지는 fact sheet뿐, 이 PDF의 재사용 조건을 제시하지
  않습니다. 보고서가 공개 배포되는 것("publicly available")과 제3자의
  복제·번역·재배포 허락은 다른 문제입니다.

따라서 베른협약 기본값(저작권 유보)으로 취급합니다. 원문 전문이 담기는
`source/`(PDF·Markdown), `state/`(번역 상태), `ko/`(번역 출력)는 커밋하지
않고(.gitignore) 설정·용어집·재현 스크립트만 커밋하는 **레시피형**으로
구성했습니다 — thebeambook 프로젝트 README의 배포 형태 구분을 따릅니다.
로컬에서 만든 번역 결과물은 저작권자(PyPy 컨소시엄 저자들)의 허락을 얻기
전에는 공개·재배포하지 마세요.

## PDF → Markdown 변환

[firecrawl/pdf-inspector](https://github.com/firecrawl/pdf-inspector)를
사용합니다 (`cargo install pdf-inspector`). 재현:

```sh
./scripts/fetch-and-convert.sh
```

- `detect-pdf --analyze` 분류 결과: **text_based**, 12쪽, OCR 필요 페이지
  없음 (`is_complex: true` — 4·6쪽 도식이 표로, 6·8·11쪽이 다단으로 감지됨).
  결과는 `source/detect.json`에 기록됩니다.
- `pdf2md --pages`로 변환해 `<!-- Page N -->` 페이지 경계를 보존합니다.
  이 마커는 번역 대상이 아니며 출력에도 그대로 남습니다.
- 변환 품질: 산문·제목·불릿·링크는 정확하게 추출됩니다. 알려진 아티팩트
  (줄바꿈 하이픈 분절, `- **` 불릿, 4·6쪽 도식 라벨의 유사 표 추출, 7·8쪽
  각주의 본문 삽입, 12쪽 연락처 표 평탄화)는 `scripts/postprocess.py`가
  최소한으로 정정합니다 — 각 정정은 적용 횟수를 단언하므로 변환 출력이
  달라지면 조용히 어긋나는 대신 실패합니다. 7쪽의 스프린트 사진은 텍스트
  추출에 포함되지 않습니다. OCR이 필요한 페이지는 없었습니다.

## 용어집

`yeokja.toml`의 `glossary = "../pypy/glossary.toml"`로 PyPy 본체 프로젝트의
용어집을 그대로 공유합니다 (경로는 프로젝트 디렉터리 기준으로 해석됩니다).
EU Report에만 필요한 용어도 같은 파일에 추가하되, pypy 문서에 이미 등장하는
단어를 추가하면 완역된 pypy 프로젝트 세그먼트가 GlossaryStale로 재번역
대상이 되므로, 추가 전에 `projects/pypy/upstream`을 grep해서 영향이 없는지
확인합니다.

## 번역 실행

저장소 루트에서:

```sh
./scripts/build.sh                                # yeokja 빌드
projects/pypy-eu-reports/scripts/fetch-and-convert.sh
yeokja -C projects/pypy-eu-reports coverage source/   # 파서 누락 확인
yeokja -C projects/pypy-eu-reports translate source/
yeokja -C projects/pypy-eu-reports status source/
```

`coverage`에서 보고되는 제외 구간은 4·6쪽 도식 펜스(의도적 제외)와 페이지
마커뿐이어야 합니다.
