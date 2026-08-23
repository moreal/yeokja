# An Infinitely Large Napkin 한국어 번역

Evan Chen의 *An Infinitely Large Napkin*을 `yeokja`로 번역하는 비공식 한국어판
프로젝트입니다. 수식, LaTeX 명령, 레이블, 인용 키와 Asymptote/TikZ 도식은 그대로
보존하고 독자에게 보이는 자연어 본문만 번역합니다.

## 구조

```text
upstream/       원문 저장소(읽기 전용 git submodule)
state/          번역 상태(*.yeokja.json, 진실의 원천)
ko/             state에서 재구성되는 번역 LaTeX(커밋하지 않음)
assets/         한국어 폰트가 포함된 Nix 빌드 정의
patches/        한국어 조판·고정 UI 문구·저작자 표시 패치
build/tree/     원문과 번역을 겹친 일회용 빌드 트리
output/pdf/     완성된 PDF
```

## 사용법

저장소 루트에서 CLI를 빌드한 뒤 이 디렉터리에서 실행합니다.

```sh
cargo build --manifest-path ../../Cargo.toml
../../target/debug/yeokja status .
../../target/debug/yeokja coverage .
../../target/debug/yeokja translate .
../../target/debug/yeokja build pdf
```

완성된 한국어판은 `output/pdf/Napkin-ko.pdf`에 생성됩니다.
[GitHub Pages에서 PDF 내려받기](https://moreal.github.io/yeokja/napkin/Napkin-ko.pdf)

번역문을 수정할 때는 `ko/`가 아니라 `state/`의 `translation` 필드를 고칩니다.
`ko/`와 `build/tree/`는 다음 실행에서 다시 만들어지는 파생 산출물입니다.

## 라이선스와 저작자 표시

원문의 본문과 PDF는 Evan Chen 및 기여자들이
[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/)으로 공개했으며,
LaTeX 소스는 [GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html)으로
공개했습니다. 이 한국어판은 기계 번역 후 검토한 2차적 저작물이고, 한국어
번역이라는 변경 사항을 명시합니다. 번역된 본문/PDF는 CC BY-SA 4.0으로,
번역된 소스는 GPL-3.0으로 배포합니다. 원저자나 기여자들이 공식 승인한
번역본이라는 뜻은 아닙니다.
