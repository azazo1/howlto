#!/usr/bin/env bash
set -euo pipefail

host="$(rustc -vV | awk '/^host:/{print $2}')"
if [[ -z "$host" ]]; then
  echo "failed to detect rustc host target" >&2
  exit 1
fi
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin) package_os=macos ;;
  Linux) package_os=linux ;;
  *)
    echo "unsupported os: $os" >&2
    exit 1
    ;;
esac

case "$arch" in
  x86_64 | amd64) package_arch=x86_64 ;;
  arm64 | aarch64) package_arch=aarch64 ;;
  *)
    echo "unsupported arch: $arch" >&2
    exit 1
    ;;
esac

echo "Building howlto for $host"
cargo build --locked --release --bins --target "$host"

if [[ "$os" == Darwin ]]; then
  echo "Signing macOS binary"
  codesign --force --sign - "target/$host/release/howlto"
fi

echo "Packaging $package_os $package_arch"
TARGET="$host" PACKAGE_OS="$package_os" PACKAGE_ARCH="$package_arch" bash scripts/package.sh
