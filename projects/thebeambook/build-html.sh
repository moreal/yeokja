#!/bin/sh
# 조립된 트리 안에서 실행됩니다 (`yeokja build`가 cwd를 트리로 잡습니다).
#
# 툴체인은 이 머신 기준입니다: HTML은 시스템 ruby 2.6의 asciidoctor,
# ditaa 다이어그램은 keg-only openjdk 21이 필요합니다.
set -e
export JAVA_HOME=/opt/homebrew/opt/openjdk@21
export PATH="$HOME/.gem/ruby/2.6.0/bin:$JAVA_HOME/bin:$PATH"

make html

# Makefile의 `rsync -R code/*/*.png site`는 심링크를 건너뜁니다(-l 없음).
# 트리에서는 코드 그림이 전부 심링크라 -L로 관통 복사를 한 번 더 합니다.
rsync -R -L code/*/*.png site
