#!/usr/bin/env bash
set -euo pipefail

internal_runner_flag="--__ftcodex-run-in-tmux"

run_codex() {
  local pause_on_failure="$1"
  local codex_bin="$2"
  local codex_status
  shift 2

  if "$codex_bin" "$@"; then
    codex_status=0
  else
    codex_status=$?
  fi

  if ((codex_status != 0)); then
    printf '\nftcodex: Codex exited with status %d.\n' "$codex_status" >&2
    printf 'Executable: %s\n' "$codex_bin" >&2
    if [[ "$pause_on_failure" == "1" && -t 0 ]]; then
      printf 'Review the output above, then press Enter to close this tmux pane. ' >&2
      IFS= read -r _ || true
    fi
  fi

  return "$codex_status"
}

if [[ "${1:-}" == "$internal_runner_flag" ]]; then
  shift
  if (($# < 2)); then
    printf 'ftcodex: internal runner requires a pause mode and Codex executable\n' >&2
    exit 2
  fi

  pause_on_failure="$1"
  if [[ "$pause_on_failure" != "0" && "$pause_on_failure" != "1" ]]; then
    printf 'ftcodex: invalid internal pause mode: %s\n' "$pause_on_failure" >&2
    exit 2
  fi
  shift

  codex_status=0
  run_codex "$pause_on_failure" "$@" || codex_status=$?
  exit "$codex_status"
fi

session_name="${FTCODEX_TMUX_SESSION:-ftcodex}"
assume_yes="${FTCODEX_ASSUME_YES:-0}"
use_alt_screen=0

while (($# > 0)); do
  case "$1" in
    -y|--yes)
      assume_yes=1
      shift
      ;;
    --session)
      if (($# < 2)); then
        printf 'ftcodex: --session requires a name\n' >&2
        exit 2
      fi
      session_name="$2"
      shift 2
      ;;
    --alt-screen)
      use_alt_screen=1
      shift
      ;;
    --)
      shift
      break
      ;;
    *)
      break
      ;;
  esac
done

codex_args=("$@")
if [[ "$use_alt_screen" != "1" ]]; then
  codex_args=(--no-alt-screen "${codex_args[@]}")
fi

if ! codex_bin="$(command -v codex)"; then
  printf 'ftcodex: codex was not found on PATH\n' >&2
  exit 127
fi

if [[ -n "${TMUX:-}" ]]; then
  exec "$codex_bin" "${codex_args[@]}"
fi

if ! command -v tmux >/dev/null 2>&1; then
  printf 'ftcodex: tmux was not found on PATH\n' >&2
  exit 127
fi

if [[ "$assume_yes" != "1" ]]; then
  if [[ ! -t 0 ]]; then
    printf 'ftcodex: current shell is not inside tmux; rerun interactively or pass --yes\n' >&2
    exit 1
  fi

  printf 'Current shell is not inside tmux. Start or attach session "%s" and launch Codex? [Y/n] ' "$session_name" >&2
  read -r answer
  case "${answer:-y}" in
    y|Y|yes|YES|Yes)
      ;;
    *)
      printf 'ftcodex: cancelled\n' >&2
      exit 1
      ;;
  esac
fi

pause_on_failure=0
if [[ -t 0 ]]; then
  pause_on_failure=1
fi

launcher_path="${BASH_SOURCE[0]}"
if [[ "$launcher_path" != */* ]]; then
  launcher_path="$(command -v -- "$launcher_path")"
fi
launcher_dir="$(cd -P -- "$(dirname -- "$launcher_path")" && pwd)"
launcher_path="$launcher_dir/$(basename -- "$launcher_path")"

printf -v codex_command '%q ' \
  "$launcher_path" "$internal_runner_flag" "$pause_on_failure" \
  "$codex_bin" "${codex_args[@]}"
exec tmux new-session -A -s "$session_name" -c "$PWD" "$codex_command"
