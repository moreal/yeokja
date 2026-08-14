# theBeamBook 한국어 번역

[The BEAM Book](https://github.com/happi/theBeamBook) (Erik Stenman 외, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/))의 한국어 번역 프로젝트입니다.

## 구조

```
upstream/       # 원본 저장소 (git submodule, 읽기 전용 — 절대 수정하지 않습니다)
state/          # 번역 상태 (*.yeokja.json) — 진실의 원천, 커밋 대상
ko/             # 번역 출력 — state/에서 재구성되는 파생 산출물, gitignore
assets/         # 한국어판 전용 추가물: PDF 테마, 나눔고딕 폰트(OFL)
patches/        # upstream 원본에 가하는 최소 수정 (Index 빈 줄, PDF 테마 전환)
build-html.sh   # HTML 빌드 (이 머신의 ruby/openjdk 툴체인 가정)
build/tree/     # 조립된 빌드 트리 — 일회용, gitignore
dist/           # 빌드 산출물 (site/) — gitignore
yeokja.toml     # 프로젝트 설정
glossary.toml   # 용어집
```

번역문을 고칠 때는 `ko/`가 아니라 `state/`의 `translation` 필드를 고친 뒤 `yeokja translate`로 출력을 재생성합니다. `ko/`를 직접 고치면 다음 실행에서 조용히 되돌아갑니다. `build/tree/`도 마찬가지로 매 조립마다 처음부터 다시 만들어집니다.

진입점 래퍼(`book-ko.asciidoc` 같은)는 없습니다. 번역 출력이 원본과 같은 상대 경로를 미러링하므로, 조립된 트리 안에서는 upstream의 Makefile이 무수정으로 한국어판을 만듭니다.

## 사용법

이 디렉터리에서 실행합니다:

```sh
yeokja status upstream/chapters      # 번역 진행 상황 + 고아 상태 보고
yeokja translate upstream/chapters   # 번역 + ko/ 재생성 (리네임 입양 포함)
yeokja build                         # 트리 조립 + make html → dist/site
```

## upstream 범프

```sh
git -C upstream fetch origin && git -C upstream checkout <새 커밋>
yeokja status upstream/chapters      # 신규/stale/고아 확인
yeokja translate upstream/chapters   # 증분 재번역
yeokja build                         # 패치가 안 맞으면 여기서 시끄럽게 실패합니다
```

서브모듈 해시, 상태 변화, 패치 수정을 한 커밋으로 묶으면 범프 전체를 커밋 하나로 되돌릴 수 있습니다.

## 배포 형태

원문이 CC BY 4.0이므로 번역 상태와 산출물을 저작자 표시와 함께 배포할 수 있는 **전체번역형** 프로젝트입니다. 원문 라이선스가 파생물 배포를 허용하지 않는 자료(ND 등)는 상태 파일에 원문이 포함되므로 커밋하지 말고, 설정과 용어집만 담는 레시피형으로 구성해야 합니다.

## 저작자 표시

한국어판은 The BEAM Book(© Erik Stenman and contributors, CC BY 4.0)을 기계 번역 후 검증한 파생물이며, 같은 조건으로 이용할 수 있습니다. 원본은 submodule로 고정된 커밋을 참조합니다.
