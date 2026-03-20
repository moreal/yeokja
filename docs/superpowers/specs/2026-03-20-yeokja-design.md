# Yeokja Design Spec

## Overview

Yeokja는 외국어 문서를 한국어로 번역하는 Rust 기반 도구이다. LLM 및 TranslateGemma 같은 번역 특화 모델을 활용하며, Markdown/Asciidoc 등 구조화된 문서를 문장 단위로 파싱하여 증분 번역을 수행한다.

## Architecture

### Crate 구조

```
yeokja/
├── crates/
│   ├── core/            # (yeokja-core) 파서 trait, 세그먼트 모델, TM, Glossary, 변경 감지
│   ├── parser-markdown/ # (yeokja-parser-markdown) Markdown 파서 구현체
│   ├── parser-asciidoc/ # (yeokja-parser-asciidoc) Asciidoc 파서 구현체 (이후 구현)
│   ├── translate/       # (yeokja-translate) LLM 연동, rate limit, provider 추상화
│   ├── cli/             # (yeokja-cli) CLI + TUI
│   └── server/          # (yeokja-server) 웹 API 서버, 데몬 모드
├── web/                  # TanStack Start + React 프론트엔드
├── yeokja.toml           # 프로젝트 설정
└── Cargo.toml            # workspace
```

디렉터리명은 prefix 없이, Cargo.toml의 패키지 name은 `yeokja-` prefix 포함.

### 책임 분리

- **core**: 포맷 파서 trait 정의, 세그먼트 모델, Translation Memory, Glossary, 변경 감지. 외부 의존 없는 순수 로직.
- **parser-markdown / parser-asciidoc**: core의 파서 trait 구현체. 플러그인 구조.
- **translate**: core에 의존. LLM API 호출, rate limit 자동 조절 담당.
- **cli**: core + translate에 의존. CLI 커맨드(clap)와 TUI(ratatui).
- **server**: core + translate에 의존. REST API(axum) 제공, 웹 UI 서빙.

## Document Parsing & Segment Model

### Parser Trait

core에서 정의:

```rust
/// TranslationMap = HashMap<SegmentId, String>
/// 번역이 없는 세그먼트는 원본 텍스트를 그대로 사용 (fallback)
trait DocumentParser {
    fn parse(&self, source: &str) -> Document;
    fn reconstruct(&self, document: &Document, translations: &TranslationMap) -> String;
}
```

### Document 구조

- `Document` → 여러 `Section` (장/챕터)
- `Section` → 여러 `Block` (paragraph, heading, list item, code block 등)
- `Block` → 여러 `Segment` (문장 단위)

### Segment

- `id`: 위치 기반 식별자 (예: `section:2/block:3/seg:1`)
- `source`: 원본 텍스트
- `source_hash`: 내용 해시 (xxHash64 사용)
- `context_hash`: `xxHash64(prev_source_hash || next_source_hash)`. 경계에서는 sentinel 값(`0`) 사용.
- `block_type`: heading / paragraph / list_item 등 (코드 블록은 번역 대상에서 제외)

### 번역 단위

- 저장/추적은 문장 단위
- 번역 요청은 블록 단위로 배치: 같은 블록에 속한 번역 대상 세그먼트들을 하나의 요청으로 묶어 보냄
- 블록 전체 텍스트를 문맥으로 제공하고, 번역 대상 세그먼트를 명시
- 서로 다른 블록의 세그먼트는 별도 요청으로 분리

## Translation Memory & Change Detection

### 파일 기반 저장

원본 문서 옆에 상태 파일 생성:

```
book/
├── chapter1.md
├── chapter1.md.yeokja.json
├── chapter2.md
└── chapter2.md.yeokja.json
```

### 상태 파일 구조

```json
{
  "version": 1,
  "source_hash": "파일 전체 해시",
  "segments": [
    {
      "id": "section:0/block:1/seg:0",
      "source": "Original sentence.",
      "source_hash": "abc123",
      "context_hash": "def456",
      "translation": "번역된 문장.",
      "glossary_snapshot": {
        "repository": "저장소"
      },
      "translated_at": "2026-03-20T10:00:00Z"
    }
  ]
}
```

status 필드는 저장하지 않는다. 매번 런타임에 평가하여 파생한다.

### 세그먼트 매칭/조정 (Reconciliation)

위치 기반 ID는 문서 구조 변경 시 밀릴 수 있다. 매칭 알고리즘:

1. 새로 파싱한 세그먼트 목록과 기존 상태 파일의 세그먼트를 비교
2. **1차: source_hash로 매칭** — 같은 내용의 세그먼트를 찾음
3. **2차: 위치 기반 tiebreaker** — 동일한 source_hash가 여러 개면 greedy nearest-match (위치가 가장 가까운 것부터 매칭, 매칭된 항목은 후보에서 제거)
4. 매칭된 세그먼트는 ID를 새 위치로 갱신하고 기존 번역을 유지
5. 매칭되지 않은 새 세그먼트 → pending
6. 매칭되지 않은 기존 세그먼트 → 즉시 제거 (별도 보존하지 않음)

### 변경 감지 로직

조정 후 각 매칭된 세그먼트에 대해:

1. `source_hash` 불일치 → stale (재번역 대상)
2. `context_hash` 불일치 → context_changed (재번역 우선순위 낮음)
3. `glossary_snapshot`과 현재 glossary 비교 → glossary_stale (재번역 대상)
4. 모두 일치 → translated (스킵)

### Glossary 연동

- 번역 시 세그먼트의 원문에 등장하는 glossary 용어를 탐지 (단어 경계 기준 substring match)
- 해당 용어의 실제 매핑값을 `glossary_snapshot`에 기록
- 평가 시 현재 glossary의 매핑값과 snapshot을 비교하여 glossary_stale 판단
- 새로 추가된 glossary 용어가 기존 세그먼트 원문에 포함되어 있으면 glossary_stale로 처리

## Glossary Management

### glossary.toml

```toml
[terms.repository]
translation = "저장소"
note = "Git 저장소를 의미"

[terms.commit]
translation = "커밋"
note = "음차 표기"
```

- `translation`: 번역어
- `note`: 선택적 메타데이터 (번역 근거, 참고사항 등)
- version 필드 없음. snapshot에 실제 매핑값을 기록하므로 불필요.

## LLM Translation

### Provider 추상화

```rust
#[async_trait]
trait TranslationProvider {
    async fn translate(&self, request: TranslateRequest) -> Result<TranslateResponse>;
}
```

### TranslateRequest

- `segments`: 같은 블록에 속한 번역 대상 문장들 (블록 단위 배치)
- `block_context`: 해당 블록(문단) 전체 텍스트
- `glossary`: 세그먼트 원문에 등장하는 용어 목록
- `source_lang`, `target_lang`

### 번역 응답 파싱

LLM에 번역 요청 시 각 세그먼트를 번호로 구분하여 전달하고, 응답도 같은 번호 형식으로 받는다:

```
프롬프트 예시:
다음 문장들을 한국어로 번역하세요. 각 번호에 대응하는 번역을 같은 번호로 응답하세요.
[1] The repository stores all history.
[2] Each commit represents a snapshot.

응답 예시:
[1] 저장소는 모든 이력을 저장합니다.
[2] 각 커밋은 스냅샷을 나타냅니다.
```

응답 파싱 실패 시 (번호 누락, 형식 불일치 등) 해당 블록 전체를 에러로 처리하고 스킵한다. 프롬프트 템플릿은 설정 파일에서 커스터마이즈 가능하도록 한다.

### Provider 구현체

- `OpenAICompatibleProvider` — OpenAI, Azure, vLLM, Ollama 등
- `AnthropicProvider` — Claude API 전용
- `GeminiProvider` — Google Gemini API 전용
- `TranslateGemmaProvider` — TranslateGemma 모델 전용 (번역 특화 오픈 모델, 고유 입출력 형식)
- 이후 필요 시 추가

### Rate Limit 자동 조절

- 429/rate limit 에러 시 exponential backoff + `retry-after` 헤더 존중
- `x-ratelimit-remaining` 등 응답 헤더를 파싱하여 요청 속도 사전 조절

## CLI / TUI / Web Interface

### CLI (clap)

```
yeokja translate ./book/          # 일괄 번역 실행
yeokja status ./book/             # 번역 진행 통계
yeokja glossary list              # 용어집 조회
yeokja glossary set term 번역     # 용어 추가/수정
yeokja serve                      # 서버 모드 시작
```

### TUI (ratatui, read-only)

- `yeokja translate ./book/ --tui`
- 실시간 번역 진행률, 현재 번역 중인 세그먼트, 에러 표시
- 파일별/섹션별 진행 상황 트리 뷰

### Web Interface

- Rust(axum) API 서버 + TanStack Start(React) 프론트엔드
- 기능:
  - 전체 문서/세그먼트 목록 및 번역 상태 조회
  - 실시간 번역 진행 상황 (SSE 또는 WebSocket)
  - 세그먼트별 수동 번역 편집
  - Glossary 관리 (CRUD)
  - 번역 시작/중지 제어

## Configuration

### yeokja.toml

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

[[sources]]
path = "docs/"
pattern = "**/*.adoc"
parser = "asciidoc"
output = "translated/{path}"

[provider]
type = "anthropic"
model = "claude-sonnet-4-20250514"
api_key_env = "ANTHROPIC_API_KEY"

[server]
port = 3000
```

- `sources`: 배열로 여러 소스 디렉터리/포맷 지정
- `output`: `{dir}`, `{stem}`, `{ext}` 템플릿 변수. `{path}`는 `sources.path` 기준 상대 경로.
- API 키는 환경변수명 참조

## Partial Output & Error Handling

- 출력 파일은 모든 세그먼트 번역 완료 후 생성. 미번역 세그먼트가 있으면 원본 텍스트로 fallback.
- 상태 파일은 atomic write (임시 파일 작성 후 rename)로 저장하여 중간 crash 시 파일 손상 방지.
- 번역 중 중단되어도 마지막으로 성공 저장된 상태부터 재실행 시 이어서 진행.
- LLM 에러(malformed 응답, 타임아웃, 5xx 등): 해당 세그먼트를 스킵하고 다음으로 진행. 에러 로그 출력. 전체 진행을 블로킹하지 않음.
- Rate limit(429): exponential backoff 후 재시도.

## Data Flow

```
원본 문서 변경
    ↓
파서가 Document → Section → Block → Segment로 분해
    ↓
각 Segment의 source_hash, context_hash 계산
    ↓
기존 상태 파일(.yeokja.json)과 비교
    ↓
┌─ hash 일치 + glossary 일치 → 스킵
├─ hash 불일치 → stale → 재번역 대상
├─ glossary snapshot 불일치 → glossary_stale → 재번역 대상
├─ context_hash만 불일치 → 낮은 우선순위 재번역
└─ 상태 없음 → pending → 번역 대상
    ↓
번역 대상 세그먼트를 블록 단위 문맥과 함께 LLM에 요청
    ↓
번역 결과 + glossary snapshot을 상태 파일에 저장
    ↓
상태 파일 기반으로 번역 문서 재구성 (reconstruct)
    ↓
출력 파일 생성
```

## Tech Stack Summary

| Component | Technology |
|-----------|-----------|
| Language | Rust |
| CLI | clap |
| TUI | ratatui |
| Web API | axum |
| Web Frontend | TanStack Start + React |
| Config | TOML |
| State Storage | JSON files |
| Markdown Parsing | pulldown-cmark (또는 유사) |
