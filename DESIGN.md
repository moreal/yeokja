# Architecture

## Overview

Yeokja는 구조화된 문서(Markdown, Asciidoc)를 문장 단위로 파싱하여 LLM 기반으로 번역하는 도구입니다. 증분 번역, 용어집 관리, 자동 품질 평가를 지원합니다.

## Crate 구조

```
yeokja/
├── crates/
│   ├── core/              # yeokja-core
│   ├── parser-utils/      # yeokja-parser-utils
│   ├── parser-markdown/   # yeokja-parser-markdown
│   ├── parser-asciidoc/   # yeokja-parser-asciidoc
│   ├── parsers/           # yeokja-parsers (파서 레지스트리)
│   ├── translate/         # yeokja-translate
│   ├── cli/               # yeokja-cli
│   └── server/            # yeokja-server
└── web/                   # TanStack Start + SolidJS
```

### 의존 관계

```
core ← parser-utils ← parser-markdown ← parsers
                     ← parser-asciidoc ← parsers
     ← translate
     ← server (+ parsers, translate)
     ← cli (+ parsers, translate, server)
```

### 각 crate의 역할

**yeokja-core** — 외부 의존 없는 순수 도메인 로직.
- `model.rs`: Document → Section → Block → Segment 계층 구조
- `parser.rs`: `DocumentParser` trait 정의, `TranslationMap` 타입
- `hash.rs`: xxHash64 기반 content_hash, context_hash
- `glossary.rs`: TOML 용어집 로딩, 단어 경계 매칭, 스냅샷 staleness 판정
- `state.rs`: `.yeokja.json` 상태 파일 I/O (atomic write)
- `reconcile.rs`: 기존 번역 상태와 새 파싱 결과 간 매칭 (greedy nearest-match)
- `change.rs`: 세그먼트별 상태 계산 (Translated, Pending, Stale, GlossaryStale, ContextChanged)
- `config.rs`: `yeokja.toml` 설정 파일 역직렬화
- `orphans.rs`: 원본이 사라진 상태 파일(고아) 탐지 — 보고만 하고 삭제하지 않습니다

**yeokja-assemble** — `[derive]` 설정이 기술하는 빌드 트리 조립 엔진.
- base 레이어 심링크 + 오버레이 겹침 + patch/generate 스텝 실행
- 임시 디렉터리에 조립해 성공 시에만 원자 교체, 실패 시 기존 트리 보존
- 산출물 반출용 `copy_dereferenced` (심링크 관통 복사)

**yeokja-parser-utils** — 파서 간 공유 유틸리티.
- `split_sentences()`: 약어와 URL 내 마침표를 고려한 문장 분리
- `make_segments()`: 텍스트를 문장 분리 후 Segment 벡터로 변환
- `normalize_inline_text()`: 여러 줄 span을 한 줄 문장으로 정규화
- `splice_reconstruct()`: span 기반 파서 공용 재구성 (원본에 번역 치환)

**yeokja-parser-markdown** — pulldown-cmark 기반 span 방식 Markdown 파서.
- `DocumentParser` trait 구현
- offset iterator로 각 블록의 인라인 콘텐츠 byte range(`Block::span`)를 기록
- 세그먼트가 링크/볼드/인라인 코드 등 raw 마크업을 그대로 포함 → LLM과 evaluator가 마크업 보존을 검증 가능
- reconstruct는 원본(`Document::source`)에서 번역 대상 span만 치환 → 코드 펜스 언어, 리스트 마커, 인용 접두사, front matter, 테이블 구조가 그대로 보존됨
- h1/h2에서 Section 분리, 코드 블록·HTML·front matter 번역 제외

**yeokja-parser-asciidoc** — 자체 라인 기반 span 방식 Asciidoc 파서.
- `DocumentParser` trait 구현
- AsciiDoc의 라인 지향 블록 구조를 스캔하며 각 블록의 텍스트 byte range를 기록
- 헤딩 마커, 리스트 불릿, admonition 라벨(NOTE: 등), 구분자(`----`, `____`)는 span에서 제외
- 저자/개정 라인, attribute entry(`:toc:`), 앵커(`[[id]]`), 블록 속성(`[source,python]`),
  주석, 테이블(`|===`)은 번역 대상에서 제외하고 그대로 보존
- reconstruct는 Markdown 파서와 동일한 splice 방식 공유 (`parser-utils::splice_reconstruct`)

**yeokja-parsers** — 파서 레지스트리.
- `select_parser()`: 소스 설정의 `parser` 필드 또는 확장자로 파서 선택
- CLI와 서버가 공유하여 파서 선택 로직 단일화

**yeokja-translate** — 번역 실행 및 품질 평가.
- `provider.rs`: `LlmProvider`/`TranslationProvider` async trait, 요청/응답 타입
- `prompt.rs`: `[N]` 번호 형식 프롬프트 생성 및 응답 파싱, 커스텀 템플릿 지원
- `factory.rs`: 설정 기반 Provider 인스턴스 생성 (CLI/서버 공유)
- `orchestrator.rs`: 파일 수집 → 조정 → 블록 번역 → 상태 저장 → 재구성 전 과정 오케스트레이션, 진행 이벤트 채널 제공
- `rate_limit.rs`: adaptive exponential backoff
- `openai_compatible.rs`, `anthropic.rs`, `gemini.rs`, `translate_gemma.rs`, `claude_code.rs`, `pi.rs`: Provider 구현
- `evaluator.rs`: `TranslationEvaluator` trait
- `evaluator_glossary.rs`, `evaluator_link.rs`, `evaluator_format.rs`, `evaluator_style.rs`: Evaluator 구현
- `pipeline.rs`: 번역 → 평가 → 재시도 자동화 루프

**yeokja-cli** — CLI 및 TUI.
- clap 기반 서브커맨드: translate, status, glossary(list/set/remove), evaluate, serve
- `--working-directory` (`-C`) 글로벌 옵션으로 작업 디렉터리 지정 가능
- ratatui 기반 실시간 번역 진행 뷰 (`--tui`): orchestrator의 진행 이벤트를 구독하여 렌더링, `q`로 취소 가능
- `serve`는 yeokja-server를 in-process로 실행

**yeokja-server** — REST API 서버.
- axum 기반, CORS 지원
- `GET /api/status`, `GET /api/segments`: 전체 진행 통계 및 세그먼트 목록(평가 이슈 포함)
- `PUT /api/segments/{file}/{id}`: 세그먼트 수동 편집 (저장 후 출력 파일 즉시 갱신)
- `GET/POST /api/glossary`, `DELETE /api/glossary/{term}`: 용어집 CRUD (glossary.toml에 영속화)
- `POST /api/translate/start`, `GET /api/translate/status`: 백그라운드 번역 실행 및 진행 조회 (동시 실행 시 409)
- `GET /api/translate/events`: 번역 진행 이벤트 SSE 스트림 (broadcast 채널 기반, 웹 대시보드가 구독)
- `POST /api/segments/{file}/{id}/evaluate`: 단일 세그먼트 수동 재평가 (결과를 상태 파일에 영속화)

## 문서 모델

```
Document (source: 원본 전문 보존)
├── Section (h1/h2 기준 분리)
│   ├── Block (paragraph, heading, list_item, code_block, ...)
│   │   ├── span: 원본 내 번역 대상 byte range (span 기반 파서)
│   │   ├── Segment (문장 단위)
│   │   │   ├── id: "section:0/block:1/seg:0"
│   │   │   ├── source: "원본 텍스트 (인라인 마크업 포함)"
│   │   │   ├── source_hash: xxHash64
│   │   │   └── block_type
│   │   └── ...
│   └── ...
└── ...
```

코드 블록, thematic break, HTML 블록, front matter는 번역 대상에서 제외됩니다.
Markdown 파서의 reconstruct는 `Document::source`에서 각 블록의 `span` 구간만
번역으로 치환하므로, 번역 대상이 아닌 모든 텍스트는 byte 단위로 보존됩니다.

## 변경 감지

세그먼트 상태는 저장하지 않고 매번 런타임에 계산합니다:

| 조건 | 상태 | 재번역 |
|------|------|--------|
| 번역 없음 | Pending | O (high) |
| source_hash 불일치 | Stale | O (high) |
| glossary snapshot 불일치 | GlossaryStale | O (high) |
| context_hash만 불일치 | ContextChanged | O (low) |
| 모두 일치 | Translated | X |

### 세그먼트 조정 (Reconciliation)

원본 문서 변경 시 기존 번역을 최대한 보존합니다:
1. `source_hash`로 1차 매칭 (같은 내용의 세그먼트)
2. 동일 해시가 여러 개면 위치 기반 greedy nearest-match
3. 매칭된 세그먼트는 기존 번역 유지, ID만 갱신
4. 매칭 실패 세그먼트는 제거 또는 pending 처리

## 번역 파이프라인

```
번역 요청 → LLM 호출 → 응답 파싱
     ↑                      ↓
     │              Evaluator 실행
     │                      ↓
     │              ┌─ 통과 → 저장
     └──────────────┤
                    └─ 실패 → 이슈를 피드백에 포함하여 재번역
                              (최대 N회, 기본 3)
```

- 기계적 검사(Glossary, Link, Format)만 재번역 트리거
- StyleEvaluator(LLM-as-judge)는 경고만 기록

### 프롬프트 형식

세그먼트를 번호로 구분하여 요청하고, 동일 형식으로 응답받습니다:

```
[1] The repository stores all history.
[2] Each commit represents a snapshot.

→

[1] 저장소는 모든 이력을 저장합니다.
[2] 각 커밋은 스냅샷을 나타냅니다.
```

TranslateGemma는 별도 형식: `<<<source>>>en<<<target>>>ko<<<text>>>...`

`[provider]`의 `prompt_template` 설정으로 프롬프트를 커스터마이즈할 수 있습니다.
사용 가능한 플레이스홀더: `{source_lang}`, `{target_lang}`, `{glossary}`,
`{feedback}`, `{context}`, `{segments}`

## 용어집 (Glossary)

`glossary.toml`에 용어와 번역어를 관리합니다. 번역 시 LLM 프롬프트에 관련 용어를 제공하고, 번역 결과에 사용된 용어의 실제 매핑값을 `glossary_snapshot`으로 기록합니다. 용어가 변경되면 해당 세그먼트를 `GlossaryStale`로 감지합니다.

## 상태 저장

원본 파일 옆에 `.yeokja.json` 사이드카 파일로 저장합니다. Atomic write (임시 파일 + rename)로 crash safety를 보장합니다. 상태 파일에는 각 세그먼트의 원문, 해시, 번역, glossary snapshot, 타임스탬프를 기록합니다.

`[project] state_dir`를 지정하면 사이드카 대신 그 디렉터리 아래에 원본의 프로젝트 상대 경로를 그대로 미러링해 저장합니다 (`upstream/chapters/x.asciidoc` → `state/upstream/chapters/x.asciidoc.yeokja.json`). 원본 트리를 읽기 전용으로 유지해야 할 때 — 예를 들어 원본이 git submodule일 때 — 사용합니다. 프로젝트 밖의 절대 경로 파일은 계속 사이드카 방식을 따릅니다.

## 설정

`yeokja.toml`로 프로젝트 설정을 관리합니다:
- `[project]`: 소스/타겟 언어, 용어집 경로, 상태 파일 디렉터리(`state_dir`)
- `[[sources]]`: 번역 대상 파일 패턴, 파서 종류, 출력 경로 템플릿
- `[provider]`: LLM 프로바이더 종류, 모델, API 키 환경변수
- `[evaluation]`: 자동 평가 활성화, 최대 재시도 횟수
- `[server]`: 서버 포트
- `[derive]`: 빌드 트리 조립 — base 레이어, 오버레이 목록, patch/generate 스텝
- `[build]`: 트리 안에서 실행할 빌드 명령과 dist로 꺼낼 산출물.
  `[build.html]`·`[build.pdf]`처럼 이름 붙은 타깃 여럿을 둘 수 있습니다

## 파생 트리 (assemble/build)

`yeokja assemble`은 base(보통 upstream submodule) 위에 오버레이(번역 미러,
프로젝트 자산)를 겹치고 patch/generate 스텝을 실행해 빌드 가능한 트리를
심링크로 조립합니다. 번역 출력이 원본과 같은 상대 경로 구조를 미러링하므로
`book-ko.asciidoc` 같은 진입점 래퍼가 필요 없고, upstream의 빌드 스크립트가
무수정으로 번역판을 만듭니다.

트리는 언제나 처음부터 다시 조립되어 원자적으로 교체되는 일회용 산출물입니다.
어떤 스텝이든 실패하면 기존 트리가 그대로 남습니다 — 청소는 수리가 아니라
재조립입니다. `require_base` 오버레이는 원본이 사라진 번역(고아)을 트리에
얹지 않고 보고합니다. `yeokja build`는 조립 후 `[build].command`를 트리에서
실행하고 선언된 산출물만 심링크를 관통 복사해 dist로 꺼냅니다.

빌드 타깃이 여럿이면 — 같은 트리에서 HTML과 PDF를 따로 뽑는 경우 —
`[build.<name>]` 서브테이블로 선언하고 `yeokja build <name>`으로 고릅니다.
타깃이 하나뿐이면 이름 유무와 무관하게 `yeokja build`만으로 충분하고,
여럿인데 이름을 안 주면 목록을 보여주며 거부합니다. 트리 조립은 타깃과
무관하게 동일합니다 — 타깃은 트리 안에서 실행할 명령의 선택일 뿐입니다.

upstream 리네임으로 고아가 된 상태는 `yeokja translate`가 전체 파일 해시 또는
세그먼트 해시 중첩(≥50%)으로 새 파일에 입양시켜 재번역 비용을 막습니다.
고아 삭제만은 자동화하지 않습니다 — `yeokja orphans --delete`로만 지웁니다.

## 기술 스택

| 영역 | 기술 |
|------|------|
| 언어 | Rust |
| CLI | clap |
| TUI | ratatui, crossterm |
| 웹 API | axum |
| 웹 프론트엔드 | TanStack Start, SolidJS, Tailwind CSS |
| 설정 | TOML |
| 상태 저장 | JSON (파일 기반) |
| Markdown 파싱 | pulldown-cmark |
| Asciidoc 파싱 | 자체 라인 기반 span 파서 |
| 해시 | xxHash64 |
