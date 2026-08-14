#!/bin/sh
# 조립된 트리 안에서 실행됩니다 (`yeokja build`가 cwd를 트리로 잡습니다).
#
# macOS 로컬 툴체인은 있을 때만 얹습니다: HTML은 시스템 ruby 2.6의
# asciidoctor, ditaa 다이어그램은 keg-only openjdk 21. CI(리눅스)에서는
# PATH의 asciidoctor/java를 그대로 씁니다.
set -e
if [ -d /opt/homebrew/opt/openjdk@21 ]; then
    export JAVA_HOME=/opt/homebrew/opt/openjdk@21
    PATH="$JAVA_HOME/bin:$PATH"
fi
if [ -d "$HOME/.gem/ruby/2.6.0/bin" ]; then
    PATH="$HOME/.gem/ruby/2.6.0/bin:$PATH"
fi
export PATH

make html

# Makefile의 `rsync -R code/*/*.png site`는 심링크를 건너뜁니다(-l 없음).
# 트리에서는 코드 그림이 전부 심링크라 -L로 관통 복사를 한 번 더 합니다.
rsync -R -L code/*/*.png site
