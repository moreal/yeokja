# Yeokja (역자)

LLM을 활용한 구조화된 문서 번역 도구. Markdown, Asciidoc 문서를 문장 단위로 파싱하여 증분 번역합니다.

## 주요 기능

- **증분 번역** — 변경된 문장만 재번역. 해시 기반 변경 감지로 불필요한 API 호출 방지
- **용어집 관리** — 일관된 용어 사용 보장. 용어 변경 시 관련 문장 자동 감지
- **자동 품질 평가** — 용어 준수, 링크 보존, 서식 유지를 자동 검증하고 실패 시 재번역
- **다중 포맷** — Markdown, Asciidoc 지원. 플러그인 구조로 확장 가능
- **다중 LLM** — OpenAI, Anthropic Claude, Google Gemini, TranslateGemma 지원
- **CLI + TUI + Web** — 일괄 실행, 실시간 진행 뷰, 웹 기반 편집

## 빠른 시작

### 설치

```sh
./scripts/build.sh
# 또는
cargo install --path crates/cli
```

### 설정

프로젝트 루트에 `yeokja.toml`을 생성합니다:

```toml
[project]
source_lang = "en"
target_lang = "ko"
glossary = "glossary.toml"

[[sources]]
path = "book/"
pattern = "**/*.md"
parser = "markdown"
output = "{dir}/{stem}.ko{ext}"

[provider]
type = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"
# 선택: 프롬프트 커스터마이즈. 플레이스홀더:
# {source_lang} {target_lang} {glossary} {feedback} {context} {segments}
# prompt_template = "..."

[translation]
# 동시에 진행할 블록 수 (기본 4). 블록 하나가 permit을 번역·평가·재시도
# 전 구간 동안 잡고 있으므로, 프로바이더 동시 요청 수의 상한이 됩니다.
concurrency = 4
```

### 테이블 컬럼 선택

레퍼런스 표에는 번역하면 안 되는 컬럼이 섞여 있습니다. `[[tables]]` 규칙으로
어떤 컬럼만 번역할지 지정할 수 있습니다:

```toml
[[tables]]
files = "chapters/*.asciidoc"        # 생략하면 모든 파일
headers = ["Instruction", "Arguments", "Explanation"]
translate = ["Explanation"]          # 나머지 컬럼은 원문 유지
```

표는 **위치가 아니라 헤더 행의 텍스트로** 찾습니다. 표가 이동하거나 행이
늘어도 규칙이 유지되고, 같은 스키마의 표가 여러 개면 규칙 하나가 전부
덮습니다. 헤더가 순서대로 들어 있으면 매칭되므로 컬럼이 추가돼도 깨지지
않습니다.

컬럼은 헤더 이름 또는 0-기반 인덱스로 지정합니다. 반대로 제외할 컬럼만
적어도 됩니다:

```toml
[[tables]]
headers = ["Instruction", "Arguments", "Explanation"]
skip = ["Arguments", 0]              # 나머지는 전부 번역
```

기본값은 "전부 번역"이고 규칙은 좁히는 방향으로만 작동합니다. 첫 행은 컬럼
이름을 정하므로 항상 번역 대상입니다. 제외된 셀은 원문이 그대로 남습니다.

Asciidoc 표는 줄이 아니라 그리드로 해석합니다. `[cols="2,3,3"]`이 컬럼 수를
정하고 셀이 순서대로 채워지므로, 한 행을 여러 줄에 나눠 써도 컬럼이 맞습니다.
`2+|`(가로 병합), `.2+|`(세로 병합), `3*|`(반복)도 자리 계산에 반영됩니다.

어떤 표가 있고 현재 규칙이 어떻게 적용되는지는 `inspect`로 확인합니다.
붙여넣을 수 있는 규칙도 함께 출력합니다:

```sh
yeokja inspect ./chapters/
```

```
chapters/ap-beam_instructions.asciidoc

  Table 2  (a rule matches this table)
    [0] Instruction       229 cells  kept as-is    allocate · allocate_heap · …
    [1] Arguments         222 cells  kept as-is    t t · t I t · …
    [2] Explanation        66 cells  translated    Allocate some words on stack · …
```

### 파서가 지나친 부분 확인

파서가 문법을 못 알아보면 오류가 나지 않습니다. 블록을 안 만들 뿐이고, 그
안의 텍스트는 번역 대상에 아예 들어가지 않습니다. 분모에 없으니 진행률은
100%로 보입니다. `coverage`가 지나친 구간을 줄 단위로 보여줍니다:

```sh
yeokja coverage ./chapters/
```

```
chapters/processes.asciidoc   50% of 1166 lines with text
    L56-120            63 lines   [source,bash]
    L716-774           59 lines   print *((Process *) 0x7fff9ed6e030)
    L1523-1581         57 lines   [source,erlang]
```

코드 블록과 주석도 함께 나옵니다. 의도적으로 번역하지 않는 것과 파서가 못
알아본 것을 구분해서 지우면, 정작 잡아야 할 오류 — 산문을 코드 블록으로
착각해 통째로 삼킨 경우 — 가 같이 사라지기 때문입니다. 대신 각 구간의 첫
줄을 미리보기로 붙였으니, `[source,erlang]`으로 시작하는 구간은 넘기고
산문으로 읽히는 긴 구간을 보시면 됩니다.

`--min-lines`로 보고 기준을 조절합니다 (기본 5줄). 선택 규칙으로 제외한
컬럼은 의도한 선택이므로 계산에 넣지 않습니다. 규칙이 적용되기 전, 파서만
측정합니다.

용어집 `glossary.toml`:

```toml
[terms.repository]
translation = "저장소"
note = "Git 저장소"

[terms.commit]
translation = "커밋"
```

### 번역

```sh
# 환경변수 설정
export ANTHROPIC_API_KEY="sk-ant-..."

# 번역 실행
yeokja translate ./book/

# 실시간 TUI 진행 뷰
yeokja translate ./book/ --tui

# 상태 확인
yeokja status ./book/
```

### 다른 디렉터리의 프로젝트 실행

`--working-directory` (`-C`) 옵션으로 `yeokja.toml`이 있는 디렉터리를 지정할 수 있습니다:

```sh
yeokja -C /path/to/project translate ./book/
yeokja -C /path/to/project status ./book/
```

### 용어집 관리

```sh
yeokja glossary list
yeokja glossary set "branch" "브랜치"
yeokja glossary remove "branch"
```

### 웹 인터페이스

```sh
# API 서버 (다음 중 하나)
yeokja serve
cargo run -p yeokja-server

# 프론트엔드 (별도 터미널)
cd web && yarn install && yarn dev
```

- 서버: http://localhost:3000
- 프론트엔드: http://localhost:5173

## 지원 Provider

| Provider | `type` | 비고 |
|----------|--------|------|
| OpenAI (호환) | `openai` | Azure, vLLM, Ollama 등 OpenAI-compatible API |
| Anthropic | `anthropic` | Claude API |
| Google Gemini | `gemini` | Gemini API |
| TranslateGemma | `translate_gemma` | 로컬 서빙 (vLLM/Ollama) |

## 변경 감지

원본 문서가 수정되면 변경된 문장만 재번역합니다:

- 문장 내용 변경 → **Stale** (재번역)
- 용어집 변경 → **GlossaryStale** (재번역)
- 인접 문장 변경 → **ContextChanged** (낮은 우선순위 재번역)
- 변경 없음 → **Translated** (스킵)

## 프로젝트 구조

```
crates/
├── core/              # 도메인 모델, 변경 감지, 용어집, 설정
├── parser-utils/      # 공용 문장 분리
├── parser-markdown/   # Markdown 파서 (pulldown-cmark)
├── parser-asciidoc/   # Asciidoc 파서 (라인 기반 span 방식)
├── translate/         # LLM 연동, 평가, 파이프라인
├── cli/               # CLI + TUI
└── server/            # REST API (axum)
web/                   # 웹 프론트엔드 (TanStack Start + SolidJS)
```

아키텍처 상세는 [DESIGN.md](DESIGN.md)를 참조해주세요.

## 개발

```sh
# 테스트
cargo test --workspace

# 린트
cargo clippy --workspace -- -D warnings

# 빌드
./scripts/build.sh
```

## 라이선스

TBD
