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
```

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
