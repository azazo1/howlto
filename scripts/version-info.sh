#!/usr/bin/env sh
set -eu

package_version=${1:-}

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "v$package_version"
  exit 0
fi

short_hash=$(git rev-parse --short=6 HEAD)

if git diff --quiet && git diff --cached --quiet; then
  clean=true
else
  clean=false
fi

exact_tag=$(git describe --tags --exact-match --abbrev=0 --match 'v*' --match '[0-9]*' 2>/dev/null || true)

if [ "$clean" = true ] && [ -n "$exact_tag" ]; then
  echo "$exact_tag"
  exit 0
fi

base_tag=$(git describe --tags --abbrev=0 --match 'v*' --match '[0-9]*' 2>/dev/null || echo "v$package_version")

if [ "$clean" = true ]; then
  echo "${base_tag}-${short_hash}"
else
  echo "${base_tag}^${short_hash}"
fi
