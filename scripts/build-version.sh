#!/usr/bin/env bash

set -euo pipefail

# 复制到项目 scripts/build-version.sh 后, 通常只需要改 TAG_PREFIX.
# 不要改后面的 tag / dirty 算法.
TAG_PREFIX="v"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

git_output() {
  local output
  if ! output="$(git "$@" 2>/dev/null)"; then
    return 1
  fi
  output="$(printf '%s' "$output" | tr -d '\r')"
  output="${output#"${output%%[![:space:]]*}"}"
  output="${output%"${output##*[![:space:]]}"}"
  [[ -n "$output" ]] || return 1
  printf '%s\n' "$output"
}

select_version_tag() {
  local tags="$1"
  local line
  if [[ -n "$TAG_PREFIX" ]]; then
    while IFS= read -r line; do
      if [[ "$line" == "$TAG_PREFIX"* ]]; then
        printf '%s\n' "$line"
        return 0
      fi
    done <<< "$tags"
  fi
  while IFS= read -r line; do
    if [[ -n "$line" ]]; then
      printf '%s\n' "$line"
      return 0
    fi
  done <<< "$tags"
  return 1
}

# 仓库一个版本 tag 都没有时的兜底版本号, 由调用方提供.
# 各语言模块给出该生态的结构化 metadata 读取命令, 在 just dist 或 CI 里先算出包版本,
# 再用 PROJECT_PACKAGE_VERSION 传进来; 不要在本脚本里正则扫清单文件.
read_package_version() {
  if [[ -n "${PROJECT_PACKAGE_VERSION:-}" ]]; then
    printf '%s\n' "$PROJECT_PACKAGE_VERSION"
    return 0
  fi

  echo "仓库里没有任何版本 tag, 且未提供 PROJECT_PACKAGE_VERSION" >&2
  echo "请先打一个版本 tag, 或按语言模块给出的命令读出包版本后传入该变量" >&2
  return 1
}

describe_latest_tag() {
  if [[ -n "$TAG_PREFIX" ]]; then
    git_output describe --tags --abbrev=0 --match "${TAG_PREFIX}*" HEAD
  else
    git_output describe --tags --abbrev=0 HEAD
  fi
}

strip_tag_prefix() {
  local display="$1"
  if [[ -n "$TAG_PREFIX" && "$display" == "$TAG_PREFIX"* ]]; then
    printf '%s\n' "${display#"$TAG_PREFIX"}"
  else
    printf '%s\n' "$display"
  fi
}

exact_tag=""
if tags="$(git_output tag --points-at HEAD)"; then
  exact_tag="$(select_version_tag "$tags" || true)"
fi

if [[ -n "$exact_tag" ]]; then
  tag="$exact_tag"
else
  tag="$(describe_latest_tag || true)"
  if [[ -z "$tag" ]]; then
    tag="${TAG_PREFIX}$(read_package_version)"
  fi
fi

commit="$(git_output rev-parse --short=7 HEAD || true)"
dirty=false
if [[ -n "$commit" ]]; then
  set +e
  git diff-index --quiet HEAD --
  status=$?
  set -e
  if [[ "$status" -eq 1 ]]; then
    dirty=true
  fi
fi

if [[ -z "$commit" ]]; then
  display="$tag"
elif [[ "$dirty" == true ]]; then
  display="${tag}^${commit}"
elif [[ -n "$exact_tag" ]]; then
  display="$tag"
else
  display="${tag}-${commit}"
fi

strip_tag_prefix "$display"
