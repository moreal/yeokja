# theBeamBook 한국어 번역

[The BEAM Book](https://github.com/happi/theBeamBook) (Erik Stenman 외, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/))의 한국어 번역 프로젝트입니다.

## 구조

```
upstream/       # 원본 저장소 (git submodule, 읽기 전용 — 절대 수정하지 않습니다)
state/          # 번역 상태 (*.yeokja.json) — 진실의 원천, 커밋 대상
ko/             # 번역 출력 — state/에서 재구성되는 파생 산출물, gitignore
yeokja.toml     # 프로젝트 설정
glossary.toml   # 용어집
```

번역문을 고칠 때는 `ko/`가 아니라 `state/`의 `translation` 필드를 고친 뒤 `yeokja translate`로 출력을 재생성합니다. `ko/`를 직접 고치면 다음 실행에서 조용히 되돌아갑니다.

## 사용법

이 디렉터리에서 실행합니다:

```sh
yeokja status upstream/chapters      # 번역 진행 상황
yeokja translate upstream/chapters   # 번역 + ko/ 재생성
```

## 배포 형태

원문이 CC BY 4.0이므로 번역 상태와 산출물을 저작자 표시와 함께 배포할 수 있는 **전체번역형** 프로젝트입니다. 원문 라이선스가 파생물 배포를 허용하지 않는 자료(ND 등)는 상태 파일에 원문이 포함되므로 커밋하지 말고, 설정과 용어집만 담는 레시피형으로 구성해야 합니다.

## 저작자 표시

한국어판은 The BEAM Book(© Erik Stenman and contributors, CC BY 4.0)을 기계 번역 후 검증한 파생물이며, 같은 조건으로 이용할 수 있습니다. 원본은 submodule로 고정된 커밋을 참조합니다.
