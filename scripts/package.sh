#!/usr/bin/env bash
set -euo pipefail

target=${TARGET:?TARGET is required}
package_os=${PACKAGE_OS:?PACKAGE_OS is required}
package_arch=${PACKAGE_ARCH:?PACKAGE_ARCH is required}
package_version=${PACKAGE_VERSION:-}

if [[ -z "$package_version" ]]; then
  cargo_version="$(
    cargo metadata --locked --no-deps --format-version 1 |
      jq -er '.packages[] | select(.name == "howlto") | .version'
  )"
  package_version="$(bash scripts/version-info.sh "$cargo_version")"
fi

archive_version="${package_version#v}"
binary="target/$target/release/howlto"
if [[ ! -x "$binary" ]]; then
  echo "missing binary: $binary" >&2
  exit 1
fi

outdir="${OUTDIR:-dist}"
mkdir -p "$outdir"
package="howlto-$archive_version-$package_os-$package_arch"
package_dir="$outdir/$package"
archive="$outdir/$package.tar.gz"
rm -rf "$package_dir" "$archive"
mkdir -p "$package_dir"
cp "$binary" "$package_dir/"
tar -C "$outdir" -czf "$archive" "$package"
rm -rf "$package_dir"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "package_version=$archive_version"
    echo "asset=$archive"
  } >> "$GITHUB_OUTPUT"
fi

echo "package_version=$archive_version"
echo "asset=$archive"
