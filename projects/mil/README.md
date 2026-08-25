# Mathematics in Lean 한국어 번역

[Mathematics in Lean](https://leanprover-community.github.io/mathematics_in_lean/)
(Jeremy Avigad, Patrick Massot)의 비공식 한국어 번역 프로젝트입니다. 원문의
실행·검증되는 Lean 예제와 연습문제는 그대로 유지하고 교재의 자연어 본문만
번역합니다.

## 원문과 라이선스

사용자가 읽는 원문 저장소는
[`leanprover-community/mathematics_in_lean`](https://github.com/leanprover-community/mathematics_in_lean)이며,
이 프로젝트는 그 저장소를 생성하는 공식 소스 저장소
[`avigad/mathematics_in_lean_source`](https://github.com/avigad/mathematics_in_lean_source)를
`upstream/` 서브모듈의 고정된 커밋으로 추적합니다.

원문 소스 저장소는 Apache License 2.0으로 배포됩니다. 교재 설정에 명시된 대로
Jeremy Avigad와 Patrick Massot가 작성한 교재 텍스트는 CC BY 4.0 조건으로 이용할
수 있습니다. 이 한국어판은 원문을 기계 번역 후 검토한 2차적 저작물이며, 번역이라는
변경 사항을 명시합니다. Lean 커뮤니티나 원저자들이 공식적으로 승인한 번역본이라는
뜻은 아닙니다.

## yeokja 처리 방식

원고의 자연어는 각 Lean 파일 안의 `/- TEXT: … TEXT. -/` 블록에
reStructuredText로 기록되어 있습니다. `mil` 파서는 텍스트 블록 밖을 같은 바이트
길이의 마스크로 바꾼 뒤 기존 RST 파서를 적용합니다. 이 방식으로 제목·문단·목록과
인라인 마크업만 번역하고 Lean 프로그램, 예제/해답 선택 지시문, 인용 코드와 주석은
바이트 단위로 보존합니다. 번역된 원고에는 upstream의 공식 `scripts/mkall.py`를
실행하여 Sphinx 입력과 실습 파일을 만든 뒤 HTML을 빌드합니다.

```text
upstream/       공식 원문 소스 저장소 (읽기 전용 git submodule)
state/          번역 상태 (*.yeokja.json, 진실의 원천)
ko/             state에서 재구성되는 번역 출력 (커밋하지 않음)
patches/        한국어판 제목·저작자 표시·변경 고지
build/tree/     원문과 번역을 겹친 일회용 빌드 트리
dist/site/      완성된 HTML
yeokja.toml     번역·조립·빌드 설정
glossary.toml   한국어 수학·Lean 용어집
```

## 사용법

저장소 루트에서 CLI를 빌드한 뒤 이 디렉터리에서 실행합니다.

```sh
cargo build --manifest-path ../../Cargo.toml
../../target/debug/yeokja status upstream/MIL
../../target/debug/yeokja translate upstream/MIL
../../target/debug/yeokja build html
```

로컬 HTML 빌드에는 Python 3과 `upstream/scripts/requirements.txt`의 패키지가
필요합니다. 번역문을 수정할 때는 `ko/`가 아니라 `state/`의 `translation` 필드를
수정합니다. `ko/`와 `build/tree/`는 다음 실행에서 다시 만들어지는 파생
산출물입니다.
