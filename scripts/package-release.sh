#!/usr/bin/env bash
set -Eeuo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$root_dir/release"
run_checks=1
run_build=1
stage_root=""

usage() {
  cat <<'EOF'
Build a platform-specific ft financial canvas release archive.

Usage:
  scripts/package-release.sh [options]

Options:
  --output <directory>  Output directory (default: ./release)
  --skip-checks         Skip TypeScript, lint, and formatting checks
  --skip-build          Package existing dist/ and native renderer binaries
  -h, --help            Show this help

Run this script natively on each release target. It does not cross-compile.
EOF
}

die() {
  printf 'package-release: %s\n' "$*" >&2
  exit 1
}

while (($# > 0)); do
  case "$1" in
    --output)
      (($# >= 2)) || die '--output requires a directory'
      output_dir="$2"
      shift 2
      ;;
    --skip-checks)
      run_checks=0
      shift
      ;;
    --skip-build)
      run_build=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

command -v bun >/dev/null 2>&1 || die 'bun was not found on PATH'
command -v tar >/dev/null 2>&1 || die 'tar was not found on PATH'

case "$(uname -s)" in
  Linux) platform="linux" ;;
  Darwin) platform="darwin" ;;
  *) die "unsupported platform: $(uname -s)" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) architecture="x64" ;;
  arm64|aarch64) architecture="arm64" ;;
  *) die "unsupported architecture: $(uname -m)" ;;
esac

cd "$root_dir"
bun install --frozen-lockfile

if [[ "$run_checks" == "1" ]]; then
  bun run check
fi

if [[ "$run_build" == "1" ]]; then
  ./ftopencode-build --force
fi

[[ -f "$root_dir/dist/index.js" ]] || die 'dist/index.js is missing; run the build first'

native_renderers=(
  'candlesticks:candlesticks'
  'chart:chart'
  'dag:dag'
  'market-table:market-table'
  'security-snapshot:security-snapshot'
)
for renderer in "${native_renderers[@]}"; do
  canvas_name="${renderer%%:*}"
  binary_name="${renderer#*:}"
  binary_path="$root_dir/canvases/$canvas_name/bin/$platform-$architecture/$binary_name"
  [[ -x "$binary_path" ]] || die "missing executable renderer: $binary_path"
done

version="$(bun -e 'console.log((await Bun.file("package.json").json()).version)')"
tag="v${version#v}"
package_name="ft-financial-canvas-$tag-$platform-$architecture"

mkdir -p -- "$output_dir"
output_dir="$(cd -- "$output_dir" && pwd)"
stage_root="$(mktemp -d "$output_dir/.financial-canvas-release.XXXXXX")"
trap '[[ -n "$stage_root" && -d "$stage_root" ]] && rm -rf -- "$stage_root"' EXIT
package_root="$stage_root/$package_name"
mkdir -p -- "$package_root"

payload=(
  Cargo.lock
  Cargo.toml
  CHANGELOG.md
  DELIVERY.md
  LICENSE
  README.md
  RELEASE.md
  bun.lock
  canvases
  dist
  docs
  ftopencode
  ftopencode-build
  package.json
  run-canvas.sh
  skills
  src
  tsconfig.json
)

for item in "${payload[@]}"; do
  [[ -e "$root_dir/$item" ]] || die "release payload is missing $item"
  cp -R -- "$root_dir/$item" "$package_root/$item"
done

find "$package_root/canvases" -type d -name target -prune -exec rm -rf -- {} +
rm -rf -- "$package_root/.memory" "$package_root/target"

shopt -s nullglob
for packaged_bin_dir in "$package_root"/canvases/*/bin/*; do
  [[ -d "$packaged_bin_dir" ]] || continue
  if [[ "${packaged_bin_dir##*/}" != "$platform-$architecture" ]]; then
    rm -rf -- "$packaged_bin_dir"
  fi
done

chmod 755 \
  "$package_root/ftopencode" \
  "$package_root/ftopencode-build" \
  "$package_root/run-canvas.sh"

archive="$output_dir/$package_name.tar.gz"
archive_tmp="$stage_root/$package_name.tar.gz"
tar -C "$stage_root" -czf "$archive_tmp" "$package_name"
mv -f -- "$archive_tmp" "$archive"

if command -v sha256sum >/dev/null 2>&1; then
  digest="$(sha256sum "$archive" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  digest="$(shasum -a 256 "$archive" | awk '{print $1}')"
else
  die 'sha256sum or shasum is required to create the checksum sidecar'
fi

printf '%s  %s\n' "$digest" "${archive##*/}" >"$archive.sha256"

printf 'Created %s\n' "$archive"
printf 'Created %s\n' "$archive.sha256"
