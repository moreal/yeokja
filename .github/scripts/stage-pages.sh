#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <artifacts-dir> <site-dir> <landing-dir>" >&2
  exit 2
fi

artifacts_dir=$1
site_dir=$2
landing_dir=$3

fail() {
  echo "error: $1" >&2
  exit 1
}

warn_preserved() {
  echo "warning: preserving published $1; $2 is unavailable" >&2
}

overlay_site() {
  artifact_name=$1
  source_name=$2
  destination_name=$3
  source_path="$artifacts_dir/$artifact_name/$source_name"

  if [ ! -d "$source_path" ]; then
    warn_preserved "$destination_name" "$artifact_name"
    return
  fi

  rm -rf "${site_dir:?}/${destination_name:?}"
  cp -R "$source_path" "$site_dir/$destination_name"
}

overlay_pypy() {
  artifact_path="$artifacts_dir/dist-pypy"
  if [ ! -d "$artifact_path/site" ] || [ ! -d "$artifact_path/rpython-site" ]; then
    warn_preserved "pypy and rpython" "dist-pypy"
    return
  fi

  rm -rf "$site_dir/pypy" "$site_dir/rpython"
  cp -R "$artifact_path/site" "$site_dir/pypy"
  cp -R "$artifact_path/rpython-site" "$site_dir/rpython"
}

overlay_download() {
  artifact_name=$1
  file_name=$2
  source_path="$artifacts_dir/$artifact_name/$file_name"

  if [ ! -f "$source_path" ]; then
    warn_preserved "napkin/$file_name" "$artifact_name"
    return
  fi

  mkdir -p "$site_dir/napkin"
  cp "$source_path" "$site_dir/napkin/$file_name"
}

test -s "$site_dir/index.html" || \
  fail "published Pages baseline is missing index.html"
test -s "$artifacts_dir/dist-devguide/site/index.html" || \
  fail "required devguide artifact is missing index.html"
test -s "$landing_dir/index.html" || fail "landing page is missing index.html"
test -s "$landing_dir/favicon.svg" || fail "landing page is missing favicon.svg"

overlay_site "dist-thebeambook" "site" "theBeamBook"
overlay_pypy
overlay_site "dist-fp-lean" "site" "fp-lean"
overlay_site "dist-tpil" "site" "tpil"
overlay_site "dist-mil" "site" "mil"
overlay_site "dist-peps" "site" "peps"
overlay_site "dist-napkin-html" "site" "napkin"
overlay_download "dist-napkin-pdf" "Napkin-ko.pdf"
overlay_download "dist-napkin-epub" "Napkin-ko.epub"
overlay_site "dist-devguide" "site" "devguide"
overlay_site "dist-rust-forge" "site" "rust-forge"

cp "$landing_dir/index.html" "$site_dir/index.html"
cp "$landing_dir/favicon.svg" "$site_dir/favicon.svg"
