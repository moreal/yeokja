# Functional Programming in Lean 한국어 번역

[Functional Programming in Lean](https://lean-lang.org/functional_programming_in_lean/)
(David Thrane Christiansen)의 비공식 한국어 번역 프로젝트입니다. 원문의
`#doc`/Verso 구조와 실행·검증되는 Lean 예제는 그대로 유지하고 자연어 본문만
번역합니다.

## 라이선스와 저작자 표시

원저작물은 Microsoft Corporation이 2023년에 공개한 판을 바탕으로 하며,
David Thrane Christiansen과 Lean FRO, LLC가 최신 Lean 및 Verso에 맞게
수정했습니다. 원문 책 안의 라이선스 고지에 따라 저작물은
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/)
조건으로 이용할 수 있습니다. 저작권 표시는
`Copyright Microsoft Corporation 2023 and Lean FRO, LLC 2023–2026`입니다.

이 저장소의 한국어판은 원문을 기계 번역 후 검토한 2차적 저작물이며, 번역이라는
변경 사항을 명시하고 같은 CC BY 4.0 조건으로 배포합니다. Lean FRO나 원저자가
공식적으로 승인한 번역본이라는 뜻은 아닙니다. 원문은 `upstream/` 서브모듈의
고정된 커밋으로 추적합니다.

## Verso 처리 방식

각 Lean 파일의 `#doc` 본문 전체를 upstream이 고정한 정확한 Verso revision의 공식
`Verso.Parser.document`로 파싱합니다. Rust에서 Verso 문법을 불완전하게 복제하지
않습니다. 공식 AST가 보존한 `SourceInfo`에서 번역 가능한 원문 byte range를
`verso-spans.json`으로 내보내고, Rust `verso` 파서는 이를 검증·소비합니다.

- 번역: `#doc` 제목, 절 제목, 문단, 목록·정의 목록, 인용문, 각주, 표의 자연어 셀
- 보존: Lean 전처리, metadata, 코드 블록, 명령과 비자연어 payload
- 검증: manifest schema와 generator, Verso revision, source hash, UTF-8 byte range

역할과 지시문도 concrete parsing 단계에서 전부 AST화됩니다. 다만 사용자 expander와
예제 코드를 실행하는 elaboration은 번역 범위 판별에 필요하지 않으므로 실행하지
않습니다. 번역은 검증된 range에만 splice되어 나머지 Lean/Verso 소스가 byte 단위로
유지됩니다. manifest가 없거나 원문·Verso revision과 어긋나면 다른 파서로 조용히
fallback하지 않고 실패합니다. 실제 원고 70개 전체의 파싱과 byte-identical 빈 번역
재구성을 코퍼스 테스트로 검증합니다. 구현 계약과 새 프로젝트 연결법은
[`crates/parser-verso/README.md`](../../crates/parser-verso/README.md)에 있습니다.

## 구조

```text
upstream/       원문 저장소 (읽기 전용 git submodule)
state/          번역 상태 (*.yeokja.json, 진실의 원천)
ko/             state에서 재구성되는 번역 출력 (커밋하지 않음)
patches/        완성된 책에 한국어판 저작자 표시를 추가하는 최소 패치
build/tree/     원문과 번역을 겹친 일회용 빌드 트리
dist/site/      완성된 HTML
yeokja.toml     번역·조립·빌드 설정
glossary.toml   한국어 용어집
```

## 사용법

저장소 루트에서 CLI를 빌드한 뒤 이 디렉터리에서 실행합니다.

```sh
cargo build --manifest-path ../../Cargo.toml
./scripts/update-verso-spans.sh
../../target/debug/yeokja status upstream/book
../../target/debug/yeokja translate upstream/book
../../target/debug/yeokja build html
```

`verso-spans.json`은 검토·커밋하는 재현 가능한 파서 산출물입니다. 평소에는 다시 만들
필요가 없지만 upstream 또는 그 Lake manifest의 Verso revision을 바꾸면 반드시 먼저
갱신해야 합니다.

대량 번역 중에는 glossary·링크·Verso 역할·문장 종결 같은 결정적 검사를 매번 실행하고,
재번역을 유발하지 않는 LLM 문체 평가는 비용과 시간을 이중으로 쓰지 않도록 끕니다.
완료 후 `../../target/debug/yeokja evaluate upstream/book --mechanical-only`로 결정적
검사만 전체 재실행하거나, 옵션을 빼고 별도 LLM 문체 검토 보고서를 생성할 수 있습니다.

번역문을 수정할 때는 `ko/`가 아니라 `state/`의 `translation` 필드를 수정합니다.
`ko/`와 `build/tree/`는 다음 실행에서 다시 만들어지는 파생 산출물입니다.

## 원문 갱신

```sh
git -C upstream fetch origin
git -C upstream checkout <새-커밋>
./scripts/update-verso-spans.sh
../../target/debug/yeokja status upstream/book
../../target/debug/yeokja translate upstream/book
../../target/debug/yeokja build html
```

서브모듈 커밋과 그에 따른 상태·파서·패치 변경을 한 커밋으로 묶으면 원문 갱신을
통째로 되돌릴 수 있습니다.
