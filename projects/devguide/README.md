# Python Developer's Guide 한국어 번역

공식 [`python/devguide`](https://github.com/python/devguide) 저장소의 RST 문서
64개를 `yeokja`와 Codex로 옮기는 비공식 한국어 번역 프로젝트입니다. Python
프로젝트나 Python Software Foundation이 제공하거나 승인한 공식 번역이 아닙니다.
원문은 CC0-1.0으로 제공되며, 이 프로젝트는 원문 커밋
`261dc2116ca81985c5c0cfc59db5a251d2c8db96`을 고정해서 사용합니다.

코드, 명령, 경로, URL, 사용자 이름, 개인 이름, 버전 및 표의 식별자 열은
독자에게 보이는 산문이 아닌 한 원문 그대로 보존합니다. `upstream/`은 읽기
전용이며 직접 수정하지 않습니다.

## 디렉터리와 책임

```text
upstream/       python/devguide 원문 서브모듈(읽기 전용)
state/          번역 상태(*.yeokja.json, 진실의 원천, 커밋 대상)
ko/             state에서 재생성되는 RST 미러(커밋하지 않음)
scripts/        한국어 빌드 준비 및 번역·HTML 완결성 검사
build/tree/     원문과 번역을 조립한 일회용 빌드 트리
dist/site/      완성된 한국어 HTML
glossary.toml   Python 및 CPython 개발 용어의 고정 번역
yeokja.toml     번역, 조립, 평가 및 빌드 설정
```

## 사용법

저장소 루트에서 CLI를 빌드하고 다음 명령을 실행합니다.

```sh
cargo build -p yeokja-cli
./target/debug/yeokja -C projects/devguide status upstream
./target/debug/yeokja -C projects/devguide translate upstream
./target/debug/yeokja -C projects/devguide evaluate upstream --mechanical-only
./target/debug/yeokja -C projects/devguide build html
```

파서가 놓친 산문과 표 선택 규칙을 감사하고, 번역 및 빌드 결과의 완결성을
검사하는 정확한 명령은 다음과 같습니다.

```sh
./target/debug/yeokja -C projects/devguide inspect upstream
./target/debug/yeokja -C projects/devguide coverage upstream --min-lines 3
python3 projects/devguide/scripts/audit.py translation
python3 projects/devguide/scripts/audit.py html
```

하나의 감사 스크립트가 제공하는 두 모드는 문제가 있으면 정렬된 진단을 출력하고
0이 아닌 상태로 종료합니다.

HTML 빌드는 Sphinx 경고를 오류로 취급합니다. 고정된 upstream의 빌드 의존성은
`projects/devguide/upstream/requirements.txt`에 있습니다.

## 전체 이력이 필요한 이유

이 서브모듈은 shallow clone으로 구성하면 안 됩니다. Sphinx의 `linklint`가
Git 이력을 사용하므로 upstream 전체 이력이 필요합니다. 새 체크아웃에서는
다음 조건을 확인합니다.

```sh
git submodule update --init projects/devguide/upstream
test "$(git -C projects/devguide/upstream rev-parse HEAD)" = 261dc2116ca81985c5c0cfc59db5a251d2c8db96
test "$(git -C projects/devguide/upstream rev-list --count HEAD)" -gt 1
```

## upstream 갱신 절차

자동으로 최신 커밋을 따라가지 않습니다. 갱신할 커밋과 라이선스·문서 범위를
먼저 검토한 뒤 `git -C projects/devguide/upstream fetch origin`과
`git -C projects/devguide/upstream checkout <검토한-커밋>`으로 gitlink를
명시적으로 바꿉니다. 이어서 64개였던 RST 파일 범위의 변화를 확인하고,
`status`, `inspect`, `coverage`, `translate`, `evaluate`, `build`와 `audit.py`의
두 검사 모드를 다시 실행합니다. 모든 결과를 검토한 뒤에만 새 gitlink와 필요한 설정·
번역 상태를 함께 커밋합니다. upstream 작업 트리에는 어떤 변경도 남기지 않습니다.
