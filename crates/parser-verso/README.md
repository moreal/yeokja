# yeokja-parser-verso

Yeokja의 Verso 지원은 Verso 문법을 Rust로 재구현하지 않습니다. 문서 프로젝트가
고정한 정확한 Lean/Verso 툴체인에서 공식 `Verso.Parser.document`를 실행하고,
그 결과인 `Lean.Doc.Syntax` AST가 보존한 원본 byte range를 manifest로 내보냅니다.
이 crate는 manifest를 엄격하게 검증한 뒤 Yeokja의 `Document` 모델로 투영합니다.

## 파싱과 elaboration의 경계

Verso 처리에는 서로 다른 두 단계가 있습니다.

1. **Concrete parsing**은 문단, 제목, 목록, 정의 목록, 인용문, 링크, 각주,
   역할, 코드 블록, metadata, 지시문을 `Lean.Doc.Syntax`로 만듭니다. 역할과
   지시문의 이름은 이 단계에서 일반적인 식별자와 인자로 파싱되므로 사용자
   확장을 임포트하거나 실행할 필요가 없습니다.
2. **Elaboration**은 `@[role_expander]`, `@[code_block_expander]`,
   `@[block_command]` 같은 사용자 코드를 실행하고 최종 `Verso.Doc`을 만듭니다.
   실행·파일 접근·예제 컴파일과 생성 콘텐츠가 개입할 수 있습니다.

번역에는 1번의 전체 AST와 source range가 필요하지만 2번은 필요하지 않습니다.
따라서 extractor는 공식 concrete parser와 그 parser를 등록하는
`Verso.Doc.Concrete`만 로드합니다. FP in Lean 자체의 expander나 예제 코드는
실행하지 않습니다.

## 데이터 흐름

```text
lake-manifest.json이 고정한 Verso revision
                  │
                  ▼
       tools/VersoSpans.lean
       Verso.Parser.document
                  │
                  ▼
       Lean.Doc.Syntax + SourceInfo
                  │ 자연어 노드 투영
                  ▼
          verso-spans.json
   (revision + source hash + byte ranges)
                  │ 엄격 검증
                  ▼
          VersoParser (Rust)
                  │ 문장 분리
                  ▼
     Document → Block → Segment
                  │
                  ▼
        원문 byte span에 splice
```

manifest 생성은 번역 프로젝트가 담당합니다. FP in Lean에서는
`projects/fp-lean/scripts/update-verso-spans.sh`가 정확한 upstream Lake 환경에서
extractor를 빌드·실행합니다.

재구성 시 upstream이 명시한 tag는 그대로 유지합니다. tag가 없는 번역된 `#doc`
제목에는 소스 경로에서 만든 안정적인 ASCII tag를 metadata에 추가하고, 영어 자동
tag를 쓰도록 지정한 절의 `tag := none`도 소스 경로와 순번 기반 tag로 바꿉니다.
Verso의 slug 변환은 한글 글자마다 같은 `___`를 사용하므로, 이 보완이 없으면 길이가
같은 서로 다른 한국어 제목이 최종 manual 조립 단계에서 충돌할 수 있습니다.

다중 페이지 HTML의 디렉터리명은 tag와 별개입니다. upstream이 `file` metadata를
명시하지 않은 번역된 `#doc`에는 영어 Lean 소스 파일명을 lowercase kebab-case로
바꾼 `file`을 추가합니다. 예를 들어 `GettingToKnow.lean`과
`DatatypesPatterns.lean`은 `/getting-to-know/datatypes-patterns/`가 됩니다. 따라서
표시 제목은 한국어로 번역해도 URL은 읽을 수 있는 안정적인 영어 경로를 유지합니다.
다중 페이지 분할 경계가 될 수 있는 일반 heading에는 공식 AST가 제공한 원문 heading
span에서 영어 단어 slug를 만들어 같은 metadata를 추가합니다. upstream의 명시적
`file` 값은 tag와 마찬가지로 그대로 보존합니다.

## AST 투영 규칙

| 공식 AST | Yeokja block | 처리 |
|---|---|---|
| `Lean.Doc.Syntax.header` | `Heading` | inline 전체 range, 실제 `#` 개수로 level 기록 |
| `para` | 문맥에 따라 `Paragraph`, `ListItem`, `BlockQuote`, `Table` | linebreak 경계로 나눈 자연어 inline range |
| `ul`, `ol`, `li` | 자식 문단을 `ListItem`으로 표시 | 중첩 구조 전체 순회 |
| `dl`, `desc` | 용어와 설명을 `ListItem`으로 표시 | 용어 inline과 설명 block 모두 처리 |
| `blockquote` | 자식 문단을 `BlockQuote`로 표시 | 중첩 목록도 유지 |
| `directive` | 자식 block 재귀 처리 | `table` 안의 자연어 셀은 `Table` |
| `footnote_ref` | 현재 문맥의 prose block | 각주 본문 번역 |
| `role`, `link`, `emph`, `bold` | 상위 prose span 안에 포함 | markup과 대상은 원문 그대로 모델에 전달 |
| `codeblock`, `metadata_block`, `command`, `link_ref` | 없음 | 원문에 byte 단위로 보존 |

inline code나 수식만 있는 문단·표 셀은 자연어가 없으므로 span을 만들지 않습니다.
이미지의 대체 텍스트는 자연어로 인식합니다. 저작권 표시는 제공된 attribution을
그대로 유지하기 위해 번역 span에서 제외합니다.

## Manifest 계약

```json
{
  "schema": 1,
  "generator": "Verso.Parser.document",
  "versoRevision": "aa447141…",
  "documents": [
    {
      "path": "upstream/book/FPLean/Intro.lean",
      "sourceHash": "11417939951322866293",
      "spans": [
        {"start": 230, "stop": 242, "kind": "heading", "level": 1}
      ]
    }
  ]
}
```

`sourceHash`는 양쪽 구현이 공유하는 UTF-8 FNV-1a 64-bit 값입니다. 보안용 해시가
아니라 원문과 range가 같은 판에서 생성됐는지 확인하기 위한 값입니다.

Rust parser는 다음 조건을 모두 강제합니다.

- schema와 generator가 지원하는 값일 것
- source 위쪽의 `lake-manifest.json`이 고정한 Verso revision과 일치할 것
- 해당 source path가 manifest에 존재할 것
- source hash가 일치할 것
- 모든 range가 UTF-8 경계 안에 있고, 정렬되어 있으며, 겹치지 않을 것
- heading에만 level이 있을 것

조건이 하나라도 어긋나면 `parse_checked`가 오류를 반환합니다. 표면 스캐너나
Markdown parser로 fallback하지 않습니다. 이 실패는 `status`, `coverage`,
`inspect`, 번역 실행 및 서버 API까지 전달됩니다. 따라서 누락된 문법이 진행률
100%로 숨는 경로가 없습니다.

## 새 Verso 프로젝트 연결

각 `[[sources]]`에 공식 extractor가 만든 manifest를 지정합니다.

```toml
[[sources]]
path = "upstream/book"
pattern = "**/*.lean"
parser = "verso"
parser_manifest = "verso-spans.json"
output = "ko/book/{path}"
```

프로젝트는 자신의 `lake-manifest.json`과 동일한 환경에서
`tools/VersoSpans.lean`을 실행해야 합니다. upstream이나 Verso revision을
갱신한 직후에는 번역 상태를 확인하기 전에 manifest부터 다시 생성합니다.

## 검증

- crate 단위 테스트: stale hash, revision 불일치, 누락 문서, 겹치는 range가
  모두 hard error인지 확인
- FP in Lean 코퍼스 테스트: 공식 manifest가 실제 70개 `#doc` 문서를 모두
  파싱하고 빈 번역 재구성이 byte identity인지 확인
- 프로젝트 CLI: manifest를 통해서만 상태와 coverage 계산
- 최종 검증: 번역 overlay를 조립한 뒤 upstream의 `lake exe fp-lean` 실행

공식 parser의 concrete syntax가 변경되면 extractor projection과 manifest schema를
함께 검토해야 합니다. 단순히 코퍼스 기대값만 낮춰서 통과시키면 안 됩니다.
