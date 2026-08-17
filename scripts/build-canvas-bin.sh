#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
workspace_dir="$root_dir/canvases"
force=0

if (($# > 1)); then
  printf 'usage: %s [--force]\n' "${0##*/}" >&2
  exit 2
fi

if (($# == 1)); then
  if [[ "$1" != "--force" ]]; then
    printf 'usage: %s [--force]\n' "${0##*/}" >&2
    exit 2
  fi
  force=1
fi

native_renderers=(
  "candlesticks:candlesticks"
  "chart:chart"
  "dag:dag"
  "market-table:market-table"
  "news-list:news-list"
  "security-snapshot:security-snapshot"
)

if ! command -v cargo >/dev/null 2>&1; then
  printf 'build-canvas-bin: cargo was not found on PATH\n' >&2
  exit 127
fi

case "$(uname -s)" in
  Linux) platform=linux ;;
  Darwin) platform=darwin ;;
  *)
    printf 'build-canvas-bin: unsupported native renderer platform: %s\n' "$(uname -s)" >&2
    exit 1
    ;;
esac
case "$(uname -m)" in
  x86_64|amd64) architecture=x64 ;;
  aarch64|arm64) architecture=arm64 ;;
  *)
    printf 'build-canvas-bin: unsupported native renderer architecture: %s\n' "$(uname -m)" >&2
    exit 1
    ;;
esac

printf 'Building all native Canvas renderers for %s-%s...\n' "$platform" "$architecture"
(
  cd "$workspace_dir"
  CARGO_INCREMENTAL=1 cargo build --workspace --release --locked
)

target_root="${CARGO_TARGET_DIR:-$workspace_dir/target}"
if [[ "$target_root" != /* ]]; then
  target_root="$workspace_dir/$target_root"
fi
for renderer in "${native_renderers[@]}"; do
  canvas_name="${renderer%%:*}"
  binary_name="${renderer#*:}"
  source_binary="$target_root/release/$binary_name"
  destination_dir="$root_dir/canvases/$canvas_name/bin/$platform-$architecture"
  destination_binary="$destination_dir/$binary_name"
  if [[ ! -f "$source_binary" ]]; then
    printf 'build-canvas-bin: Cargo did not produce %s\n' "$source_binary" >&2
    exit 1
  fi
  mkdir -p "$destination_dir"
  if [[ "$force" == "0" && -f "$destination_binary" ]] && cmp -s "$source_binary" "$destination_binary"; then
    printf 'Packaged %s binary is up to date.\n' "$canvas_name"
  else
    install -m 755 "$source_binary" "$destination_binary"
    printf 'Updated %s\n' "$destination_binary"
  fi
done

# Non-Rust renderers may still provide their own build hooks.
shopt -s nullglob
for canvas_builder in "$root_dir"/canvases/*/build; do
  canvas_dir="$(dirname -- "$canvas_builder")"
  if [[ ! -x "$canvas_builder" || -f "$canvas_dir/Cargo.toml" ]]; then
    continue
  fi
  if [[ "$force" == "1" ]]; then "$canvas_builder" --force; else "$canvas_builder"; fi
done
