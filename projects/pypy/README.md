# PyPy 문서 한국어 번역

[PyPy](https://github.com/pypy/pypy) 저장소의 두 문서 트리(`pypy/doc/*.rst`와
`rpython/doc/*.rst`, [MIT](https://github.com/pypy/pypy/blob/main/LICENSE))의
한국어 번역 프로젝트입니다. RPython 문서는 같은 저장소의 별도 Sphinx
프로젝트(rpython.readthedocs.io)입니다.

## 구조

```
upstream/       # 원본 저장소 (git submodule, 읽기 전용 — 절대 수정하지 않습니다)
ko/             # 번역 출력 (yeokja가 생성, 손으로 고치지 않습니다 — 상태 파일을 고칠 것)
state/          # 번역 상태 (*.yeokja.json, 진실의 원천)
build/tree/     # 조립된 빌드 트리 (yeokja assemble이 만들며, 커밋하지 않습니다)
dist/           # 빌드 결과물 (yeokja build)
```

## 범위

`upstream/pypy/doc/`와 `upstream/rpython/doc/`의 RST 문서 전체가 소스로
등록되어 있으나, 릴리스 노트(`release-*.rst`, `whatsnew-*.rst`, 136개)는
번역하지 않습니다 — 번역되지 않은 파일은 오버레이 트리에서 영어 원문 그대로
남습니다. 실제 번역 대상은 PyPy 본문 약 50개 + RPython 27개입니다.

## 빌드

공식 빌드 환경이 **Python 2.7**입니다(`.readthedocs.yaml`) — `conf.py`의
`pypyconfig` 확장이 py2 문법인 `pypy.config`/`rpython.config`를 임포트해
설정 문서를 생성하기 때문입니다. macOS에서는 brew의 `pypy`(2.7)로 환경을
만듭니다:

```sh
brew install pypy
pypy -m ensurepip
pypy -m pip install --user 'virtualenv==16.7.12'
pypy -m virtualenv ~/.venvs/pypy-doc-py2
~/.venvs/pypy-doc-py2/bin/pip install 'sphinx<2' docutils==0.11 \
    sphinx-issues==1.2.0 'sphinx_rtd_theme<1' py sphinx-affiliates
PATH=~/.venvs/pypy-doc-py2/bin:$PATH yeokja build html
```

rpython 임포트가 플랫폼 프로브(.o 컴파일)를 발동하며 소스 루트를 realpath로
검증하므로, 링크 트리에서는 단언이 깨집니다. `yeokja.toml`의 generate 스텝이
`rpython/`을 실사본으로 구체화해 해결하고, 그 구체화가 ko 오버레이를
덮어쓰므로 rpython/doc 번역본을 되얹습니다. 성공 시 경고는 PyPy 문서 4건
(Mercurial 정보 등)·RPython 문서 0건이며, 산출물은 `dist/site`(PyPy)와
`dist/rpython-site`(RPython, Pages의 /rpython/ 경로 — 트리의 rpython 패키지
디렉터리와 이름이 겹치지 않는 별도 이름)입니다.
