# Devguide GitHub Pages 배포 설계

## 목표

`main`에 커밋된 Python Developer's Guide 한국어 번역을
`https://moreal.github.io/yeokja/devguide/`에 자동 배포한다. 다른 번역
프로젝트의 일시적인 미번역·빌드 실패가 devguide의 새 배포를 막지 않아야 하며,
실패한 기존 프로젝트의 마지막 정상 공개본을 새 Pages 배포에서 삭제해서도 안
된다.

완료 조건은 다음과 같다.

- `main` push가 devguide의 64개 상태 파일에서 한국어 RST를 재구성한다.
- devguide의 `status --check upstream`과 경고-오류 Sphinx 빌드가 통과해야만
  배포가 진행된다.
- 성공한 프로젝트는 새 산출물로 교체되고, 실패한 기존 프로젝트는 현재 공개본을
  유지한다.
- Pages 루트에서 devguide 한국어판으로 이동할 수 있다.
- 배포 후 `/devguide/`가 HTTP 200을 반환하고 한국어 제목을 포함한다.

## 현재 문제

`.github/workflows/pages.yml`은 `main` push마다 모든 기존 프로젝트를 하나의
matrix로 빌드하고, matrix 전체가 성공해야 `stage`와 `deploy`를 실행한다. 현재
MIL 9개, PEPs 704개, PyPy 3개의 새 세그먼트가 번역되지 않아 각 프로젝트의
완결성 검사가 실패하고, 후속 두 잡은 건너뛰어진다. 또한 devguide는 matrix와
Pages 스테이징 목록에 등록되어 있지 않다.

단순히 실패한 matrix 항목을 무시하고 성공한 산출물만 새 Pages artifact에 넣으면
실패한 프로젝트의 공개 디렉터리가 통째로 사라진다. GitHub Pages Actions 배포는
부분 갱신이 아니라 업로드한 artifact 전체로 사이트를 교체하기 때문이다.

## 채택한 배포 모델

### 공개본 기반 보존 후 성공 산출물 오버레이

`stage` 잡은 현재 공개된 `https://moreal.github.io/yeokja/` 트리를 `_site`의
기준선으로 내려받는다. 이어서 이번 실행에서 성공해 artifact를 만든 프로젝트만
정해진 하위 경로에 덮어쓴다.

- 성공한 기존 프로젝트: 기존 하위 디렉터리를 제거하고 새 artifact로 교체한다.
- 실패한 기존 프로젝트: artifact가 없으므로 기준선의 마지막 공개본을 유지한다.
- devguide: 새 artifact가 반드시 존재해야 한다. 없으면 스테이징을 실패시키고
  기존 Pages 배포를 그대로 둔다.
- 루트 `site/index.html`과 `site/favicon.svg`: 저장소의 최신 파일로 항상
  교체한다.

기준선 다운로드는 고정된 자체 Pages HTTPS 주소만 사용한다. 다운로드 명령이 일부
깨진 링크를 만났더라도 계속할 수 있지만, `_site/index.html`을 확보하지 못하면
즉시 실패한다. 이렇게 하면 외부 네트워크 장애나 최초 기준선 부재가 빈 사이트
배포로 이어지지 않는다.

## Workflow 변경

`rebuild` matrix에 다음 devguide 항목을 추가한다.

```yaml
- project: devguide
  target: html
  toolchain: python3-devguide
  artifact: dist-devguide
  artifact_path: projects/devguide/dist
```

devguide 잡은 고정된 `projects/devguide/upstream` submodule만 받고, Sphinx의
Git timestamp 계산에 필요한 전체 이력으로 unshallow한 뒤 Python과 `uv`를
설치한다. 다른 프로젝트처럼 상태 파일마다 CLI를 반복 호출하지 않고 다음 두
명령으로 프로젝트 전체를 한 번에 재구성하고 검증한다.

```bash
../../target/release/yeokja translate upstream
../../target/release/yeokja status --check upstream
```

그 뒤 기존 `[build.html]` 설정으로 `yeokja build html`을 실행한다. 이 설정은
Sphinx의 `--fail-on-warning --keep-going`을 사용하므로 devguide 경고도 배포를
막는다.

기존 matrix 항목에는 job 수준의 조건부 `continue-on-error`를 적용한다.
`matrix.project != 'devguide'`인 실패는 artifact 부재로 표현되고, devguide 실패는
matrix 전체를 실패시켜 후속 배포를 막는다. `fail-fast: false`는 유지하여 한 기존
프로젝트의 실패가 devguide 빌드를 취소하지 않게 한다.

## 스테이징 스크립트

artifact 오버레이는 `.github/scripts/stage-pages.sh`로 분리한다. 스크립트는
artifact 루트와 `_site` 경로를 인자로 받고 다음 책임만 가진다.

1. 기준선과 devguide artifact의 `site/index.html` 존재를 확인한다.
2. 디렉터리형 사이트는 성공 artifact가 있을 때만 정확한 목적 경로를 교체한다.
3. PyPy artifact의 `site`와 `rpython-site`를 각각 `/pypy`, `/rpython`에
   함께 반영한다.
4. Napkin HTML은 `/napkin`을 교체하고, PDF·EPUB artifact가 있으면 같은
   디렉터리에 파일을 갱신한다.
5. devguide를 `/devguide`에 필수로 반영한다.
6. 저장소 루트 랜딩 페이지와 favicon을 마지막에 복사한다.

경로 목록은 스크립트 안에 고정한다. matrix 입력이나 외부 문자열을 삭제 경로로
사용하지 않는다.

## 랜딩 페이지

`site/index.html`의 “옮긴 글” 목록에 “Python 개발자 가이드” 항목을 추가한다.
링크는 상대 경로 `devguide/`를 사용하고, 비공식 한국어 기계 번역임과 원문
`python/devguide`, CC0-1.0 라이선스를 표시한다.

## 오류 처리와 안전성

- devguide 번역 상태, 빌드 또는 artifact가 실패하면 새 Pages 배포는 일어나지
  않는다.
- 기존 프로젝트 실패는 경고로 남지만 마지막 공개본을 보존한다.
- 현재 공개 Pages 기준선을 가져오지 못하면 빈 artifact를 만들지 않고 실패한다.
- 성공 artifact로 교체할 때만 미리 정한 정확한 하위 디렉터리를 삭제한다.
- Actions는 번역 제공자를 호출하지 않는다. 커밋된 devguide `state/`가 완전하므로
  재구성 시 모델 요청은 0건이어야 한다.
- 동시 Pages 실행은 기존 `concurrency` 설정을 유지하여 서로의 배포를 덮어쓰지
  않는다.

## 검증

다음 검증을 모두 통과해야 push한다.

1. 스테이징 테스트가 실패한 기존 artifact를 생략해도 기준선 디렉터리가 유지됨을
   확인한다.
2. 성공 artifact는 이전 파일을 제거하고 새 파일로 교체됨을 확인한다.
3. devguide artifact나 기준선 index가 없으면 스테이징이 실패함을 확인한다.
4. PyPy/RPython과 Napkin 다중 산출물이 올바른 목적지에 배치됨을 확인한다.
5. workflow 정적 검사가 devguide matrix, Python/uv 설정, 조건부 실패 허용,
   기준선 다운로드, 스테이징 스크립트 호출을 확인한다.
6. `shellcheck .github/scripts/stage-pages.sh`가 통과한다.
7. `cargo test --workspace`와 devguide Python 테스트가 통과한다.
8. 로컬 devguide 재구성·상태 검사·경고-오류 HTML 빌드·HTML 감사가 통과한다.
9. push 후 Pages workflow가 성공하고 deploy URL이 생성된다.
10. 공개 `/devguide/`가 HTTP 200과 “Python 개발자 가이드”를 반환한다.

## 범위 밖

이번 변경은 MIL, PEPs, PyPy의 새 세그먼트를 번역하지 않는다. 이 세 프로젝트의
마지막 공개본을 보존하면서 devguide 배포를 독립시키는 것이 목적이다. 장기적으로
기존 프로젝트의 상태를 갱신하면 그 다음 성공한 Pages 실행에서 새 artifact가
자동으로 기존 공개본을 교체한다.
