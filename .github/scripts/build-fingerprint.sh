#!/usr/bin/env bash
#
# 프로젝트의 빌드 산출물을 결정하는 입력만 모아 하나의 지문으로 접습니다.
# 파서 crate가 바뀌었다는 사실 자체는 무효화 근거가 아닙니다 — 문장 분리와
# 재구성 결과가 그대로면 ko/가 바이트 단위로 동일하고, 그러면 이 지문도
# 그대로입니다. 여기서 읽는 것은 오직 실제 렌더링 입력뿐입니다:
#   - upstream 서브모듈 HEAD (+ 더티 여부)
#   - 프로젝트 트리의 git 추적 파일 (state/ 제외 — 상태 변화는 ko/에 반영됨)
#   - yeokja translate가 이미 재구성해 둔 ko/ 의 내용
#   - 빌드 타깃 이름
#
# 이 스크립트는 프로젝트 밖을 참조하지 않는 `[derive] base = "upstream"` +
# 오버레이(ko/, assets/ 등) 구성을 전제합니다. 그 구성을 벗어나는 프로젝트가
# 생기면 이 스크립트도 함께 고쳐야 합니다.

set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <project-dir> <target> [--daily]" >&2
  exit 2
fi

project_dir=$1
target=$2
daily=${3:-}

if [ ! -d "$project_dir" ]; then
  echo "error: no such project directory: $project_dir" >&2
  exit 1
fi

fingerprint_input() {
  echo "target:$target"

  if [ -d "$project_dir/upstream/.git" ] || [ -f "$project_dir/upstream/.git" ]; then
    echo "base-head:$(git -C "$project_dir/upstream" rev-parse HEAD)"
    echo "base-dirty:$(git -C "$project_dir/upstream" status --porcelain)"
  else
    echo "base-head:none"
  fi

  echo "tracked:"
  git ls-files -s -- "$project_dir" ":!:$project_dir/state" | LC_ALL=C sort

  echo "ko:"
  if [ -d "$project_dir/ko" ]; then
    find "$project_dir/ko" -type f -exec sha256sum {} + | LC_ALL=C sort
  else
    echo "none"
  fi

  if [ "$daily" = "--daily" ]; then
    echo "date:${YEOKJA_FINGERPRINT_DATE:-$(date -u +%F)}"
  fi
}

fingerprint_input | sha256sum | cut -c1-16
