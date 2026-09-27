#!/usr/bin/env bash

# 语言无关的归档 helper, 复制到项目 scripts/archive.sh.
#
# 由各语言的 scripts/dist.sh 调用, 按统一规则拼接产物名并压缩:
#   PROJECT-VERSION-PLATFORM-ARCH[-VARIANT].EXT
# 平台无关的托管产物 (jar, 框架依赖 dll 程序集等) 设 PLATFORM_INDEPENDENT=1, 命名为:
#   PROJECT-VERSION[-VARIANT].EXT
#
# 必需的环境变量:
#   PROJECT_NAME           产物名前缀, 例如 dida
#   PROJECT_BUILD_VERSION  构建版本号, 例如 v0.1.2 或 v0.1.2-a1b2c3d
# 可选的环境变量:
#   TARGET_PLATFORM        linux / macos / windows, 缺省按当前平台判断
#   TARGET_ARCH            x86_64 / aarch64, 缺省按当前架构判断
#   VARIANT                setup / portable, 只用于同一平台同一架构有多种分发形态的桌面应用
#   PLATFORM_INDEPENDENT   设为 1 时按平台无关规则命名
#   ARCHIVE_EXT            覆盖扩展名, 缺省 windows 用 zip, 其他平台用 tar.gz
#   DIST_DIR               产物输出目录, 缺省 dist
#
# 用法:
#   PROJECT_NAME=dida PROJECT_BUILD_VERSION=v0.1.2 scripts/archive.sh <staging-dir> [artifact...]
# 不传 artifact 时打包 staging 目录下的全部内容.

set -euo pipefail

project_name="${PROJECT_NAME:-}"
if [[ -z "$project_name" ]]; then
  echo "缺少 PROJECT_NAME, 无法拼接产物名" >&2
  exit 1
fi

version="${PROJECT_BUILD_VERSION:-}"
if [[ -z "$version" ]]; then
  echo "缺少 PROJECT_BUILD_VERSION, 无法拼接产物名" >&2
  exit 1
fi

if (( $# < 1 )); then
  echo "用法: PROJECT_NAME=<name> PROJECT_BUILD_VERSION=<version> $0 <staging-dir> [artifact...]" >&2
  exit 1
fi

staging="$1"
shift

if [[ ! -d "$staging" ]]; then
  echo "staging 目录不存在: $staging" >&2
  exit 1
fi

detect_platform() {
  case "$(uname -s)" in
    Linux) printf 'linux\n' ;;
    Darwin) printf 'macos\n' ;;
    MINGW* | MSYS* | CYGWIN*) printf 'windows\n' ;;
    *)
      echo "无法识别平台 $(uname -s), 请显式设置 TARGET_PLATFORM" >&2
      return 1
      ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64 | amd64) printf 'x86_64\n' ;;
    arm64 | aarch64) printf 'aarch64\n' ;;
    *)
      echo "无法识别架构 $(uname -m), 请显式设置 TARGET_ARCH" >&2
      return 1
      ;;
  esac
}

variant_suffix=""
if [[ -n "${VARIANT:-}" ]]; then
  variant_suffix="-${VARIANT}"
fi

platform_suffix=""
if [[ "${PLATFORM_INDEPENDENT:-0}" == "1" ]]; then
  ext="${ARCHIVE_EXT:-zip}"
else
  platform="${TARGET_PLATFORM:-$(detect_platform)}"
  arch="${TARGET_ARCH:-$(detect_arch)}"
  platform_suffix="-${platform}-${arch}"
  if [[ "$platform" == "windows" ]]; then
    ext="${ARCHIVE_EXT:-zip}"
  else
    ext="${ARCHIVE_EXT:-tar.gz}"
  fi
fi

dist_dir="${DIST_DIR:-dist}"
mkdir -p "$dist_dir"
archive="${dist_dir}/${project_name}-${version}${platform_suffix}${variant_suffix}.${ext}"
archive_abs="$(cd "$(dirname "$archive")" && pwd)/$(basename "$archive")"

shopt -s nullglob
artifacts=()
if (( $# > 0 )); then
  for artifact in "$@"; do
    if [[ ! -e "$staging/$artifact" ]]; then
      echo "staging 目录缺少产物: $staging/$artifact" >&2
      exit 1
    fi
    artifacts+=("$artifact")
  done
else
  staged=("$staging"/*)
  if (( ${#staged[@]} == 0 )); then
    echo "staging 目录为空, 没有可打包的内容: $staging" >&2
    exit 1
  fi
  for entry in "${staged[@]}"; do
    artifacts+=("${entry##*/}")
  done
fi

rm -f "$archive"
case "$ext" in
  tar.gz | tgz)
    tar -czf "$archive" -C "$staging" "${artifacts[@]}"
    ;;
  zip)
    if ! command -v zip > /dev/null 2>&1; then
      echo "缺少 zip 命令, Windows 产物请在 PowerShell 中改用 scripts/archive.ps1" >&2
      exit 1
    fi
    (cd "$staging" && zip -qr "$archive_abs" "${artifacts[@]}")
    ;;
  *)
    echo "不支持的扩展名: $ext" >&2
    exit 1
    ;;
esac

echo "已生成 $archive"
