# Theorem Proving in Lean 4 한국어 번역

[Theorem Proving in Lean 4](https://lean-lang.org/theorem_proving_in_lean4/)
(Jeremy Avigad, Leonardo de Moura, Soonho Kong, Sebastian Ullrich)의 비공식
한국어 번역 프로젝트입니다. 원문의 `#doc`/Verso 구조와 실행·검증되는 Lean
예제는 그대로 유지하고 자연어 본문만 번역합니다.

## 라이선스와 변경 고지

원저작물은 Lean Community의 기여와 함께 Apache License 2.0으로 공개되어
있습니다. 이 한국어판은 원문을 기계 번역 후 검토한 2차적 저작물이며, 각 수정된
문서에 한국어 번역이라는 변경 사항을 표시하고 원문 저장소와 라이선스를
연결합니다. Lean FRO나 원저자들이 공식적으로 승인한 번역본이라는 뜻은 아닙니다.
원문은 `upstream/` 서브모듈의 고정된 커밋으로 추적합니다.

## 구조와 처리 방식

`fp-lean` 프로젝트와 같은 공식 Verso AST 추출기를 사용합니다. 각 Lean 파일의
`#doc` 본문을 upstream이 고정한 Verso revision의 `Verso.Parser.document`로
파싱하여 번역 가능한 byte range만 `verso-spans.json`에 기록합니다. Lean 코드,
metadata, Verso 명령과 역할 payload는 보존됩니다.

```text
upstream/       원문 저장소 (읽기 전용 git submodule)
state/          번역 상태 (*.yeokja.json, 진실의 원천)
ko/             state에서 재구성되는 번역 출력 (커밋하지 않음)
patches/        한국어판 변경·라이선스 고지
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

번역문을 수정할 때는 `ko/`가 아니라 `state/`의 `translation` 필드를 수정합니다.
`ko/`와 `build/tree/`는 다음 실행에서 다시 만들어지는 파생 산출물입니다.
