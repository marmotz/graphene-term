#!/usr/bin/env bash
set -Eeuo pipefail

backend=${1:-}
case "$backend" in
  x11|wayland) ;;
  *)
    printf 'Usage: %s <x11|wayland>\n' "$0" >&2
    exit 2
    ;;
esac

binary=${GRAPHENE_BIN:-target/debug/graphene}
if [[ ! -x "$binary" ]]; then
  printf 'Graphene executable is missing or not executable: %s\n' "$binary" >&2
  exit 2
fi

for command in bash cat chmod find head mktemp pgrep rm seq sleep timeout; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 2
  }
done

case "$backend" in
  x11)
    : "${DISPLAY:?Run this probe inside an X11 session, e.g. xvfb-run}"
    for command in xdotool; do
      command -v "$command" >/dev/null || {
        printf 'Required command not found: %s\n' "$command" >&2
        exit 2
      }
    done
    ;;
  wayland)
    for command in grim sway swaymsg python3 wtype; do
      command -v "$command" >/dev/null || {
        printf 'Required command not found: %s\n' "$command" >&2
        exit 2
      }
    done
    python3 -c 'from wayland_automation import Mouse' >/dev/null 2>&1 || {
      printf 'Install wayland-automation (see docs/development.md)\n' >&2
      exit 2
    }
    ;;
esac

artifact_root=${GRAPHENE_E2E_ARTIFACTS:-"${TMPDIR:-/tmp}/graphene-e2e-artifacts"}
mkdir -p "$artifact_root"
tmp=$(mktemp -d "$artifact_root/$backend.XXXXXX")
app_pid=
shell_pid=
sway_pid=

stop_pid() {
  local pid=$1
  [[ -n "$pid" ]] || return 0
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 40); do
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
}

stop_app() {
  [[ -n "$app_pid" ]] || return 0

  shell_pid=$(pgrep -P "$app_pid" | head -n1 || true)
  if [[ -n "$shell_pid" ]]; then
    stop_pid "$shell_pid"
  fi
  stop_pid "$app_pid"
  app_pid=
  shell_pid=
}

capture_failure() {
  case "$backend" in
    x11)
      if command -v import >/dev/null; then
        import -window root "$tmp/screenshot.png" 2>/dev/null || true
      fi
      ;;
    wayland)
      command -v grim >/dev/null && grim "$tmp/screenshot.png" 2>/dev/null || true
      ;;
  esac
}

cleanup() {
  local result=$?
  trap - EXIT INT TERM
  if ((result == 0)); then
    stop_app
    stop_pid "$sway_pid"
    rm -rf -- "$tmp"
  else
    printf 'E2E artifacts kept at %s\n' "$tmp" >&2
    if [[ -f "$tmp/graphene.log" ]]; then
      cat "$tmp/graphene.log" >&2
    fi
    if [[ -f "$tmp/sway.log" ]]; then
      cat "$tmp/sway.log" >&2
    fi
    capture_failure
    stop_app
    stop_pid "$sway_pid"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p \
  "$tmp/home" \
  "$tmp/config/graphene" \
  "$tmp/state" \
  "$tmp/data" \
  "$tmp/cache" \
  "$tmp/runtime"
chmod 700 "$tmp/runtime"

export HOME="$tmp/home"
export XDG_CONFIG_HOME="$tmp/config"
export XDG_STATE_HOME="$tmp/state"
export XDG_DATA_HOME="$tmp/data"
export XDG_CACHE_HOME="$tmp/cache"
export XDG_RUNTIME_DIR="$tmp/runtime"
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export WGPU_BACKEND=gl
export GRAPHENE_E2E_PROMPT_READY="$tmp/prompt-ready"
export GRAPHENE_E2E_COMMAND_RESULT_FILE="$tmp/command-result"
export SHELL="$tmp/shell"
export GRAPHENE_SHELL_INTEGRATION=0

cat >"$tmp/config/graphene/config.toml" <<'EOF'
[updates]
check = false

[window]
width = 960
height = 600
EOF

cat >"$tmp/shell" <<'EOF'
#!/bin/sh
printf 'GRAPHENE_E2E_READY\n'
export PS1=
export PROMPT_COMMAND=': > "$GRAPHENE_E2E_PROMPT_READY"; unset PROMPT_COMMAND'
exec /bin/bash --noprofile --norc -i
EOF
chmod +x "$tmp/shell"

case "$backend" in
  x11)
    unset WAYLAND_DISPLAY SWAYSOCK
    export WINIT_UNIX_BACKEND=x11
    ;;
  wayland)
    unset DISPLAY WAYLAND_DISPLAY SWAYSOCK
    export WINIT_UNIX_BACKEND=wayland
    export WLR_BACKENDS=headless
    export WLR_RENDERER=pixman
    export WLR_LIBINPUT_NO_DEVICES=1
    export WLR_HEADLESS_OUTPUTS=1
    cat >"$tmp/sway.conf" <<'EOF'
output HEADLESS-1 mode 1280x800
EOF
    sway --unsupported-gpu -d -c "$tmp/sway.conf" >"$tmp/sway.log" 2>&1 &
    sway_pid=$!

    for _ in $(seq 1 200); do
      wayland_socket=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' -print -quit)
      sway_socket=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'sway-ipc.*.sock' -print -quit)
      if [[ -n "$wayland_socket" && -n "$sway_socket" ]]; then
        export WAYLAND_DISPLAY=${wayland_socket##*/}
        export SWAYSOCK=$sway_socket
        if swaymsg -t get_outputs >/dev/null 2>&1; then
          break
        fi
      fi
      if ! kill -0 "$sway_pid" 2>/dev/null; then
        printf 'Sway exited before its virtual Wayland session was ready\n' >&2
        exit 1
      fi
      sleep 0.05
    done
    if [[ -z "${WAYLAND_DISPLAY:-}" ]] || ! swaymsg -t get_outputs >/dev/null 2>&1; then
      printf 'Timed out waiting for the virtual Wayland session\n' >&2
      exit 1
    fi
    ;;
esac

"$binary" >"$tmp/graphene.log" 2>&1 &
app_pid=$!

wait_for_file() {
  local file=$1
  local description=$2
  for _ in $(seq 1 200); do
    if [[ -f "$file" ]]; then
      if ! kill -0 "$app_pid" 2>/dev/null; then
        printf 'Graphene exited before %s\n' "$description" >&2
        return 1
      fi
      return 0
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      printf 'Graphene exited before %s\n' "$description" >&2
      return 1
    fi
    sleep 0.05
  done
  printf 'Timed out waiting for %s\n' "$description" >&2
  return 1
}

wait_for_file "$GRAPHENE_E2E_PROMPT_READY" 'the controlled shell prompt'

focus_wayland_window() {
  python3 - <<'PY'
from wayland_automation import Mouse

with Mouse() as mouse:
    mouse.click(100, 60)
PY
}

case "$backend" in
  x11)
    window=$(timeout 15 xdotool search --sync --onlyvisible --classname graphene | head -n1)
    xdotool windowfocus "$window"
    ;;
  wayland)
    focus_wayland_window
    ;;
esac

# The command is typed through the GUI driver, so the result proves the keyboard
# input reached the shell through Graphene's window and PTY. The arithmetic makes the
# expected text absent from the typed characters. The leading spaces tolerate the
# first virtual-key events being consumed while a compositor registers its keyboard.
typed_command="   printf 'E2E_RESULT_%s\\n' \$((6 * 7)) >\"\$GRAPHENE_E2E_COMMAND_RESULT_FILE\""
expected=E2E_RESULT_42
case "$backend" in
  x11)
    xdotool type --clearmodifiers --delay 20 "$typed_command"
    xdotool key --clearmodifiers Return
    ;;
  wayland)
    # One wtype process keeps a single virtual keyboard for the text and Return; its
    # initial sleep lets Graphene receive the new keymap before the first key.
    wtype -s 500 -d 20 "$typed_command" -P Return -p Return
    ;;
esac

wait_for_file "$GRAPHENE_E2E_COMMAND_RESULT_FILE" 'the typed command to run'
result=
for _ in $(seq 1 100); do
  result=$(<"$GRAPHENE_E2E_COMMAND_RESULT_FILE")
  [[ "$result" == "$expected" ]] && break
  sleep 0.05
done
if [[ "$result" != "$expected" ]]; then
  printf 'Expected command result %s; got: %s\n' "$expected" "$result" >&2
  exit 1
fi

printf '%s GUI smoke test passed: typed command produced %s\n' \
  "$backend" "$result"
stop_app
stop_pid "$sway_pid"
