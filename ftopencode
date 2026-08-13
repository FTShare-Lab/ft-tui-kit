#!/usr/bin/env bash
set -euo pipefail

internal_runner_flag="--__ftopencode-run-in-tmux"

run_opencode() {
  local pause_on_failure="$1"
  local opencode_bin="$2"
  local opencode_status
  shift 2

  if "$opencode_bin" "$@"; then
    opencode_status=0
  else
    opencode_status=$?
  fi

  if ((opencode_status != 0)); then
    printf '\nftopencode: OpenCode exited with status %d.\n' "$opencode_status" >&2
    printf 'Executable: %s\n' "$opencode_bin" >&2
    if [[ "$pause_on_failure" == "1" && -t 0 ]]; then
      printf 'Review the output above, then press Enter to close this tmux pane. ' >&2
      IFS= read -r _ || true
    fi
  fi

  return "$opencode_status"
}

if [[ "${1:-}" == "$internal_runner_flag" ]]; then
  shift
  if (($# < 2)); then
    printf 'ftopencode: internal runner requires a pause mode and OpenCode executable\n' >&2
    exit 2
  fi

  pause_on_failure="$1"
  if [[ "$pause_on_failure" != "0" && "$pause_on_failure" != "1" ]]; then
    printf 'ftopencode: invalid internal pause mode: %s\n' "$pause_on_failure" >&2
    exit 2
  fi
  shift

  opencode_status=0
  run_opencode "$pause_on_failure" "$@" || opencode_status=$?
  exit "$opencode_status"
fi

session_name="${FTOPENCODE_TMUX_SESSION:-ftopencode}"
assume_yes="${FTOPENCODE_ASSUME_YES:-0}"

while (($# > 0)); do
  case "$1" in
    -y|--yes)
      assume_yes=1
      shift
      ;;
    --session)
      if (($# < 2)); then
        printf 'ftopencode: --session requires a name\n' >&2
        exit 2
      fi
      session_name="$2"
      shift 2
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

if ! opencode_bin="$(command -v opencode)"; then
  printf 'ftopencode: opencode was not found on PATH\n' >&2
  exit 127
fi

if [[ -n "${TMUX:-}" ]]; then
  exec "$opencode_bin" "$@"
fi

if ! command -v tmux >/dev/null 2>&1; then
  printf 'ftopencode: tmux was not found on PATH\n' >&2
  exit 127
fi

if [[ "$assume_yes" != "1" ]]; then
  if [[ ! -t 0 ]]; then
    printf 'ftopencode: current shell is not inside tmux; rerun interactively or pass --yes\n' >&2
    exit 1
  fi

  printf 'Current shell is not inside tmux. Start or attach session "%s" and launch OpenCode? [Y/n] ' "$session_name" >&2
  read -r answer
  case "${answer:-y}" in
    y|Y|yes|YES|Yes)
      ;;
    *)
      printf 'ftopencode: cancelled\n' >&2
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

printf -v opencode_command '%q ' \
  "$launcher_path" "$internal_runner_flag" "$pause_on_failure" "$opencode_bin" "$@"
exec tmux new-session -A -s "$session_name" -c "$PWD" "$opencode_command"
