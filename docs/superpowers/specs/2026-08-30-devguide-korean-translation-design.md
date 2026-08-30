# Python Devguide 한국어 번역 설계

## 목표

공식 [`python/devguide`](https://github.com/python/devguide) 저장소를 고정된
원문으로 사용하고, yeokja와 Codex로 독자에게 보이는 reStructuredText 문서
전체를 한국어로 증분 번역한다. 번역 상태는 검토·수정·재생성이 가능한 형태로
버전 관리하고, 번역된 Sphinx 사이트를 원문과 같은 문서 구조로 빌드한다.

완료 상태는 다음 조건을 모두 만족해야 한다.

- `python/devguide`의 고정 커밋이 읽기 전용 서브모듈로 등록되어 있다.
- 사이트를 구성하는 모든 RST 원문과 include 조각이 yeokja 번역 대상이다.
- 번역 제공자는 Codex이며 모델은 `gpt-5.6-sol`로 고정되어 있다.
- 모든 대상 세그먼트가 번역 상태이고 번역 출력이 재생성된다.
- 한국어 Sphinx HTML 빌드가 경고를 오류로 취급한 상태에서 성공한다.
- 내부 링크, RST 구조, 번역 누락 및 고아 상태 검사가 통과한다.

## 범위

`projects/devguide/`를 하나의 독립 번역 프로젝트로 추가한다. 원문의 루트 RST,
주제별 디렉터리의 RST, `include/`의 RST 조각을 포함하여 `upstream/**/*.rst`
전체를 번역한다. 코드 블록, 명령, 경로, 역할(role), 참조 대상, URL처럼 RST
파서가 구조로 식별한 부분은 원문을 보존한다.

원문의 Python·JavaScript·CSS·이미지 같은 비문서 자산과 Sphinx 확장은 그대로
재사용한다. HTML 템플릿에 포함된 문구나 Sphinx 테마의 공통 UI 문자열은
yeokja의 RST 입력 범위가 아니므로 이 프로젝트에서 별도 번역 소스로 확장하지
않는다. Sphinx의 `language = "ko"` 설정으로 제공되는 표준 UI 번역은 사용한다.

원문은 CC0-1.0이므로 번역 상태와 번역 산출물을 저장소에서 배포할 수 있다.
한국어판 README에는 비공식 기계 번역임과 원문 위치, 고정 커밋, 라이선스를
명시한다.

## 접근 방식

### 채택: RST 미러 오버레이

기존 `projects/pypy`와 `projects/peps`가 사용하는 구조를 따른다. 원문 서브모듈
위에 `ko/` 번역 미러를 겹치고, 한국어 빌드 설정을 결정적인 생성 단계로
적용한다. yeokja의 현재 RST 파서, 상태 저장, 고아 탐지, 조립 및 빌드
기능을 그대로 활용하므로 새 제품 기능이 필요 없다.

### 제외: Sphinx gettext/PO 카탈로그

Sphinx의 표준 국제화 경로이지만 yeokja는 PO 카탈로그를 직접 생성하거나
증분 병합하지 않는다. 이를 채택하면 RST 세그먼트 상태와 PO 메시지 식별자를
연결하는 별도 하위 시스템이 필요해 이번 목표보다 범위가 커진다.

### 제외: 번역된 원문 트리 전체 복제

구현은 단순하지만 원문 자산을 중복 저장하고 upstream 갱신 시 변경 경계를
흐린다. `state/`를 진실의 원천으로 삼는 기존 프로젝트 운영 방식과도 맞지
않는다.

## 디렉터리와 책임

```text
projects/devguide/
├── upstream/       python/devguide 원문 서브모듈(읽기 전용)
├── state/          번역 상태(*.yeokja.json, 진실의 원천, 커밋 대상)
├── ko/             state에서 재생성되는 RST 미러(커밋하지 않음)
├── scripts/        한국어 빌드 준비 및 완결성 검사
├── build/tree/     원문과 번역을 조립한 일회용 트리
├── dist/site/      완성된 한국어 HTML
├── glossary.toml   CPython 개발 용어의 고정 번역
├── yeokja.toml     번역·조립·빌드 설정
└── README.md       사용법, 범위, 라이선스, 유지보수 절차
```

`state/`만 사람이 수정할 수 있는 번역 데이터다. `ko/`, `build/`, `dist/`는
언제든 재생성할 수 있으므로 `.gitignore`에서 제외한다. `upstream/`은 절대
수정하지 않는다.

## 번역 설정

하나의 RST 소스 규칙이 `upstream/**/*.rst`를 찾아 `ko/{path}`에 같은 상대
경로로 출력한다. 상태 파일은 `state/`에 저장한다. 번역 용어집에는 최소한
CPython, core developer, contributor, pull request, issue tracker, buildbot,
interpreter, bytecode, standard library, regression, backport, deprecation,
release manager처럼 문서 전반에서 반복되는 개발 용어를 고정한다.

제공자 설정은 다음 의미를 갖는다.

```toml
[provider]
type = "pi"
model = "gpt-5.6-sol"
base_url = "openai-codex"
```

이는 기존 PEP 번역 프로젝트와 같은 Codex 경로다. 기계 평가를 켜고 구조·링크·
용어 검증 실패 시 최대 3회 재시도한다. 동시성과 배치 크기는 기존 PEP 번역의
검증된 값에서 시작하되 실제 Codex 처리량과 실패율을 보며 낮출 수 있다. 모델과
제공자 종류는 처리량 조정으로 바꾸지 않는다.

## 조립과 빌드

yeokja의 파생 트리는 `upstream/`을 기반으로 만들고 `ko/`를
`require_base = true`로 겹친다. 이 조건은 upstream에서 삭제된 파일의 오래된
번역이 사이트에 남는 일을 막는다. `scripts/prepare.py`는 매번 새로 만들어진
조립 트리의 원본 `conf.py`를 확인한 뒤 `language = "ko"`와 한국어 HTML 제목을
추가한다. 원본 설정 전체를 복제하지 않으므로 upstream의 Sphinx 변경을 그대로
따르며, 예상한 설정 파일이 없거나 이미 충돌하는 언어 설정이 있으면 조용히
덮어쓰지 않고 실패한다. 원문 링크와 비공식 번역 고지는 프로젝트 README와
사이트 제목에 명시한다.

HTML 빌드는 조립된 트리 안에서 upstream이 고정한 요구 사항을 설치한 환경으로
`make html SPHINXOPTS="-W --keep-going"`을 실행한다. 가능한 오류를 한 번에
수집하면서 모든 경고를 실패로 취급하고, 성공한 `_build/html`을 `site`로 옮겨
yeokja가 `dist/site`로 내보내게 한다. 의존성 설치법은 README에 재현 가능하게
기록한다.

## 데이터 흐름

1. upstream 서브모듈을 고정 커밋으로 체크아웃한다.
2. `yeokja status upstream`으로 대상 파일과 신규·변경·고아 세그먼트를 확인한다.
3. `yeokja translate upstream`이 RST를 파싱하고 Codex로 번역해 `state/`를
   갱신한 뒤 `ko/`를 재생성한다.
4. `yeokja coverage upstream`으로 파서가 지나친 산문 후보를 감사한다.
5. `yeokja build html`이 원문, 한국어 RST, 한국어 설정을 조립하고 Sphinx를
   실행해 `dist/site`를 만든다.
6. 완결성 스크립트가 소스/상태/출력 대응, 미번역 세그먼트, 고아 상태와 HTML
   내부 링크를 검사한다.

upstream 갱신 시에는 서브모듈 커밋을 먼저 바꾼 뒤 같은 흐름을 반복한다.
yeokja의 해시 기반 변경 감지를 사용하므로 바뀐 문장과 필요한 문맥만 다시
번역한다.

## 오류 처리

- 번역 응답이 RST 구조, 링크 또는 용어 규칙을 깨면 자동 평가 피드백과 함께
  재시도하고, 한도를 넘긴 세그먼트는 실패 상태로 남겨 전체 완료를 막는다.
- 파서 coverage에서 긴 산문 구간이 누락되면 번역을 계속하기 전에 RST 파서
  지원 여부를 확인하고, 필요한 경우 파서 수정은 별도 검증 가능한 변경으로
  추가한다.
- Sphinx 경고는 빌드 실패로 취급한다. upstream 자체의 불가피한 경고가 있다면
  원인을 기록하고 정확한 메시지만 허용하며 광범위한 경고 비활성화는 하지 않는다.
- upstream에서 사라진 파일의 상태는 고아로 보고하며 `require_base`가 빌드
  트리에 포함하지 않는다. 실제 삭제 여부를 검토한 후 상태 파일을 정리한다.
- Codex 호출이 일시적으로 실패하면 yeokja의 재시도와 증분 상태를 이용해 같은
  작업을 이어가며, 이미 성공한 번역을 다시 시작하지 않는다.

## 검증

구현과 번역 완료 시 다음 증거를 모두 확보한다.

1. `git submodule status projects/devguide/upstream`이 고정 커밋을 출력한다.
2. `yeokja status upstream`이 모든 대상 세그먼트를 translated로 보고하고 신규,
   stale, 실패, 고아 상태를 0으로 보고한다.
3. `yeokja coverage upstream` 결과를 검토해 누락된 독자용 산문이 없음을 확인한다.
4. 소스 RST 파일 집합과 `state/`, `ko/` 대응 검사가 누락을 0으로 보고한다.
5. 용어·링크·마크업 기계 평가가 모든 번역 세그먼트에 통과한다.
6. `yeokja build html`이 새 조립 트리에서 성공하고 `dist/site/index.html`을 만든다.
7. 생성된 HTML 전체의 내부 링크 검사와 한국어 텍스트 표본 검사가 통과한다.
8. `git status`에 생성물이나 upstream 내부 수정이 나타나지 않는다.

좁은 표본 빌드나 일부 파일의 번역 성공만으로 전체 목표 완료를 선언하지 않는다.

## 구현 경계

우선 기존 yeokja 기능만으로 프로젝트를 구성한다. 실제 원문을 파싱하거나 빌드할
때 devguide 문법을 지원하지 못하는 증거가 나온 경우에만 필요한 parser-rst 또는
조립 기능을 최소 범위로 확장하고 회귀 테스트를 추가한다. 번역 사이트 배포,
검색 인덱스 서버, 지속적 자동 upstream 갱신은 이번 목표에 포함하지 않는다.
