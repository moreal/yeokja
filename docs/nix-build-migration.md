# CI 빌드의 Nix 이주 타당성 조사

빌드 지문 캐싱(`.github/scripts/build-fingerprint.sh`)이 "입력이 그대로면 CI
잡을 건너뛴다"를 다룬다면, 이 문서는 다른 축인 "각 빌드가 자기 머신에서도
재현 가능한가"를 다룹니다. 둘은 서로 배타적이지 않습니다 — 지문 게이트는
Nix로 옮긴 프로젝트에도 그대로 유효합니다(캐시 적중 시 잡 자체를 건너뛰므로).

**이 문서는 조사 결과이며, 코드 변경을 담고 있지 않습니다.**

## 조사 방법

`.github/workflows/pages.yml`의 `rebuild` 잡 matrix(11개 항목)와 각 프로젝트의
`yeokja.toml` `[build]` 섹션을 대조해, 툴체인 설치 스텝이 실제로 무엇을
요구하는지와 `[build.<target>].command`가 빌드 중 무엇에 접근하는지를 확인했습니다.

## 프로젝트별 현황

| 프로젝트 | 타깃 | 현재 툴체인 | nixpkgs로 고정 가능? | 순수 derivation을 막는 요소 |
|---|---|---|---|---|
| napkin | html/pdf/epub | Nix(이미 flake) + LaTeXML(HTML/EPUB은 `assets/build-html.py`가 non-Nix 경로) | 예 — PDF는 이미 이주됨 | HTML/EPUB 타깃은 `nix develop`으로 셸만 빌리고 `python3 build-html.py`를 그 안에서 돌리는 방식이라, derivation이 아니라 명령 실행 — 셸 자체는 재현 가능해도 출력은 derivation 캐시의 대상이 아님 |
| rust-forge | html | Rust(dtolnay) + mdbook(GitHub Releases 바이너리 다운로드) + blacksmith(`RUN_BLACKSMITH=1`) | mdbook·Rust 툴체인 자체는 가능 | **`RUN_BLACKSMITH=1`이 빌드 중 `static.rust-lang.org`에서 최신 stable/beta/nightly 채널 정보를 가져옴** — 입력이 같아도 결과가 날마다 달라지므로 순수 derivation과 원리적으로 충돌. FOD(fixed-output derivation)로 감싸도 해시가 계속 어긋남 |
| thebeambook | html | Ruby 3.3 + gem(asciidoctor 등) + Graphviz + fonts-noto-cjk | 예 | 없음 — Ruby 젬 셋이 작고 버전 고정이 쉬움. **이주 난이도 하** |
| devguide | html | Python 3.12 + uv | 예 | contributors 표가 git 히스토리 전체(`--unshallow`)를 요구 — Nix derivation은 격리된 소스 트리를 받으므로 `.git` 전체를 입력에 포함시켜야 함(가능은 하나 `src = ./.` 규칙과 상충, `builtins.fetchGit`으로 우회 가능) |
| peps | html | Python 3.12 + pip(requirements.txt) | 예 | devguide와 동일한 이유로 하지 않음(단, peps는 `unshallow`를 쓰지 않음 — 실제로는 장벽 없음). **이주 난이도 하** |
| mil | html | Python 3.12 + pip(requirements.txt) | 예 | 없음. **이주 난이도 하** |
| pypy | html(pypy+rpython) | PyPy 2.7(actions/setup-python) + `sphinx<2` `docutils==0.11` `sphinx-issues==1.2.0` `sphinx_rtd_theme<1` py `sphinx-affiliates` + Graphviz | 어려움 | **Python 2.7 자체가 nixpkgs unstable에서 제거됨**(2023년경). `docutils==0.11`처럼 PyPI 인덱스 기준으로도 10년 이상 된 핀이 많아 nixpkgs의 python2Packages와 버전이 맞지 않을 가능성이 높음. `pypy.config`가 소스 루트를 realpath로 검증하는 커스텀 프로브가 있어(derive.step 주석 참조) sandbox 안에서 그대로 통과할지 별도 검증 필요 |
| fp-lean | html | elan(런타임에 Lean 툴체인 자동 설치) + Python 3.10.4 | 어려움 | **elan이 `lean-toolchain` 파일을 보고 빌드 중 네트워크로 툴체인을 내려받음** — `lake exe`가 두 개의 서로 다른 lean-toolchain(book/examples)을 각각 설치. `lean4-nix`(nix 커뮤니티 오버레이) 같은 우회가 필요하고, Lake 자체의 의존성 잠금과 Nix derivation 경계가 잘 맞물리는지 검증이 더 필요 |
| tpil | html | fp-lean과 동일 | 어려움 | fp-lean과 동일 |

## 순수 derivation을 막는 요소 정리

1. **빌드 중 네트워크 접근** — `rust-forge`(release 채널 조회). FOD로 감싸도
   출력이 시간에 따라 변하므로 해시가 안정적이지 않습니다. 지문 게이트 쪽에서
   이미 `--daily`로 다루고 있는 것과 같은 근본 원인입니다.
2. **런타임 툴체인 다운로드** — `fp-lean`·`tpil`의 elan. `lean4-nix` 등으로
   Lean 툴체인 자체를 derivation 입력으로 만들 수 있지만, 별도 검증이 필요합니다.
3. **오래된 언어 런타임 핀** — `pypy`의 Python 2.7 + 사문화된 패키지 버전들.
   nixpkgs에서 이미 제거된 런타임이라 별도 오버레이나 vendoring이 필요합니다.
4. **git 히스토리 의존** — `thebeambook`·`devguide`의 contributors 표.
   `--unshallow`로 전체 히스토리를 받는데, Nix derivation의 `src = ./.`는
   보통 얕은 스냅샷이라 `builtins.fetchGit { shallow = false; }` 같은 우회가
   필요합니다. thebeambook은 실제로 `unshallow: true`가 붙어 있어 이 조건에
   해당하지만 툴체인 자체(Ruby)는 쉬우므로 난이도를 깎지는 않았습니다.

## 이주 우선순위 제안

**전면 이주가 아니라, 값싼 것부터 옮기는 순서**를 제안합니다.

1. **mil, thebeambook** (난이도 하) — 툴체인이 작고 고정이 쉽습니다. 여기서
   프로젝트별 `flake.nix` 작성 패턴과 CI 통합 방식(napkin의 `nix develop` +
   `cachix/install-nix-action`)을 정립한 뒤 나머지에 재사용합니다.
2. **devguide, peps** (난이도 하, git 히스토리만 확인 필요) — mil/thebeambook과
   같은 패턴에 `fetchGit` 우회만 추가하면 됩니다.
3. **napkin의 html/epub 타깃** (난이도 중) — 이미 flake가 있으니 `nix develop`
   +명령 실행 방식을 `nix build`로 정식 derivation화합니다. PDF 타깃이 참고
   사례입니다.
4. **fp-lean, tpil** (난이도 상) — `lean4-nix` 검증이 선행 과제입니다. 두
   프로젝트가 같은 우회를 공유하므로 한 번 검증하면 둘 다 이득입니다.
5. **pypy** (난이도 상, 별도 트랙) — Python 2.7 자체가 nixpkgs에 없어
   `nixpkgs-unstable` 대신 오래된 nixpkgs 리비전을 핀하거나, PyPy 문서 빌드를
   위한 python2 오버레이를 별도로 관리해야 합니다. 이득 대비 비용이 가장 큰
   항목이라 후순위로 둡니다.
6. **rust-forge** — 빌드 자체가 매일 변하는 외부 데이터에 의존하므로
   **순수 derivation으로 이주할 대상이 아닙니다.** 지문 게이트의 `--daily`로
   이미 다루고 있는 것으로 충분합니다.

## 지문 게이트와의 관계

프로젝트를 Nix로 옮기더라도 `.github/scripts/build-fingerprint.sh`가 계산하는
지문(upstream HEAD + 추적 파일 + `ko/` 내용)은 그대로 유효합니다. 캐시가
맞으면 `nix build`조차 실행하지 않고 잡을 건너뛰므로, Nix 이주는 캐시가
빗나갔을 때(즉, 실제로 다시 빌드해야 할 때)의 재현성과 속도를 개선하는
것이지 지문 게이트를 대체하지 않습니다.
