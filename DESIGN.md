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
│   ├── translate/         # yeokja-translate
│   ├── cli/               # yeokja-cli
│   └── server/            # yeokja-server
└── web/                   # TanStack Start + SolidJS
```

### 의존 관계

```
core ← parser-utils ← parser-markdown
                     ← parser-asciidoc
     ← translate
     ← cli (+ parser-markdown, translate)
     ← server (+ parser-markdown, translate)
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

**yeokja-parser-utils** — 파서 간 공유 유틸리티.
- `split_sentences()`: 약어를 고려한 문장 분리
- `make_segments()`: 텍스트를 문장 분리 후 Segment 벡터로 변환

**yeokja-parser-markdown** — pulldown-cmark 기반 Markdown 파서.
- `DocumentParser` trait 구현
- h1/h2에서 Section 분리, 코드 블록 번역 제외, heading level 보존

**yeokja-parser-asciidoc** — asciidoc-parser 라이브러리 기반 Asciidoc 파서.
- `DocumentParser` trait 구현
- 동일한 Section/Block/Segment 모델로 변환

**yeokja-translate** — 번역 실행 및 품질 평가.
- `provider.rs`: `TranslationProvider` async trait, 요청/응답 타입
- `prompt.rs`: `[N]` 번호 형식 프롬프트 생성 및 응답 파싱
- `rate_limit.rs`: adaptive exponential backoff
- `openai_compatible.rs`, `anthropic.rs`, `gemini.rs`, `translate_gemma.rs`: Provider 구현
- `evaluator.rs`: `TranslationEvaluator` trait
- `evaluator_glossary.rs`, `evaluator_link.rs`, `evaluator_format.rs`, `evaluator_style.rs`: Evaluator 구현
- `pipeline.rs`: 번역 → 평가 → 재시도 자동화 루프

**yeokja-cli** — CLI 및 TUI.
- clap 기반 서브커맨드: translate, status, glossary, evaluate, serve
- `--working-directory` (`-C`) 글로벌 옵션으로 작업 디렉터리 지정 가능
- ratatui 기반 실시간 번역 진행 뷰 (`--tui`)
- `provider_factory.rs`: 설정 기반 Provider 인스턴스 생성

**yeokja-server** — REST API 서버.
- axum 기반, CORS 지원
- `/api/status`, `/api/segments`, `/api/glossary` 등 엔드포인트
- 세그먼트 수동 편집 (`PUT /api/segments/{file}/{id}`)

## 문서 모델

```
Document
├── Section (h1/h2 기준 분리)
│   ├── Block (paragraph, heading, list_item, code_block, ...)
│   │   ├── Segment (문장 단위)
│   │   │   ├── id: "section:0/block:1/seg:0"
│   │   │   ├── source: "원본 텍스트"
│   │   │   ├── source_hash: xxHash64
│   │   │   └── block_type
│   │   └── ...
│   └── ...
└── ...
```

코드 블록, thematic break, HTML 블록은 번역 대상에서 제외됩니다.

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

## 용어집 (Glossary)

`glossary.toml`에 용어와 번역어를 관리합니다. 번역 시 LLM 프롬프트에 관련 용어를 제공하고, 번역 결과에 사용된 용어의 실제 매핑값을 `glossary_snapshot`으로 기록합니다. 용어가 변경되면 해당 세그먼트를 `GlossaryStale`로 감지합니다.

## 상태 저장

원본 파일 옆에 `.yeokja.json` 사이드카 파일로 저장합니다. Atomic write (임시 파일 + rename)로 crash safety를 보장합니다. 상태 파일에는 각 세그먼트의 원문, 해시, 번역, glossary snapshot, 타임스탬프를 기록합니다.

## 설정

`yeokja.toml`로 프로젝트 설정을 관리합니다:
- `[project]`: 소스/타겟 언어, 용어집 경로
- `[[sources]]`: 번역 대상 파일 패턴, 파서 종류, 출력 경로 템플릿
- `[provider]`: LLM 프로바이더 종류, 모델, API 키 환경변수
- `[evaluation]`: 자동 평가 활성화, 최대 재시도 횟수
- `[server]`: 서버 포트

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
| Asciidoc 파싱 | asciidoc-parser |
| 해시 | xxHash64 |
