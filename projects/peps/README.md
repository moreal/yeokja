# Python Enhancement Proposals 한국어 번역

공식 [`python/peps`](https://github.com/python/peps) 저장소의 PEP를 `yeokja`로
옮기는 비공식 한국어 번역 프로젝트입니다. 원문의 PEP 번호, 저자, 상태, 유형,
날짜와 링크는 그대로 보존하고 제목 값과 reStructuredText 본문만 번역합니다.

## 문서 구조

PEP 파일은 YAML front matter가 아니라 Internet Message Format(RFC 2822 계열)의
헤더로 시작합니다. `pep` 파서는 `PEP`, `Author`, `Status`, `Type`, 날짜 같은
기계 판독 필드를 번역하지 않습니다. 독자에게 보이는 `Title` 값과 첫 빈 줄 뒤의
RST 본문만 기존 RST 파서에 넘깁니다. 원문의 Copyright/License 절도 법적 문구가
달라지지 않도록 바이트 단위로 보존합니다.

```text
upstream/       python/peps 원문 저장소(읽기 전용 서브모듈)
state/          번역 상태(*.yeokja.json, 진실의 원천)
ko/             state에서 재구성되는 번역 RST(커밋하지 않음)
overrides/      한국어 사이트 템플릿과 스타일
scripts/        PEP별 라이선스 감사와 안전한 빌드 준비
build/tree/     원문과 번역을 겹친 일회용 빌드 트리
dist/site/      완성된 HTML
```

## PEP별 라이선스 처리

PEP마다 마지막 Copyright/License 절을 별도로 검사합니다. 현재 고정한 원문에는
총 737개 PEP가 있습니다.

- 704개: 퍼블릭 도메인 또는 퍼블릭 도메인/CC0-1.0 이중 허용
- 5개: Open Publication License v1.0 이상(OPUBL-1.0)
- 27개: 재배포·번역을 허용하는 명시적 라이선스를 확인하지 못함
- PEP 401: 원문이 인용·복제·수정·배포를 명시적으로 금지

앞의 709개만 번역합니다. 나머지 28개는 원문 본문을 복제하지 않고, 라이선스상
번역을 제공할 수 없다는 자체 작성 안내와 Python.org의 공식 원문 링크만
배포합니다. OPUBL-1.0 문서는 번역본임을 표시하고 수정자·수정일·변경 내용,
원저자, 수정되지 않은 원문의 위치, 비승인 고지를 페이지 상단에 넣습니다.

감사 결과와 번역 제외 목록이 설정과 일치하는지는 다음 명령으로 검증합니다.

```sh
python3 scripts/licenses.py audit --check-config
```

## 사용법

저장소 루트에서 CLI를 빌드한 뒤 이 디렉터리에서 실행합니다.

```sh
cargo build --manifest-path ../../Cargo.toml
../../target/debug/yeokja status upstream/peps
../../target/debug/yeokja translate upstream/peps
../../target/debug/yeokja build html
```

공식 PEP 빌드 의존성은 `upstream/requirements.txt`에 있습니다. 번역문을 수정할
때는 `ko/`가 아니라 `state/`의 `translation` 필드를 수정합니다.
