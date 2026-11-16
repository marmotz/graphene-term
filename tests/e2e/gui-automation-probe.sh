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

for command in bash mktemp pgrep timeout; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 2
  }
done

case "$backend" in
  x11)
    : "${DISPLAY:?Run this probe inside an X11 session, e.g. Xvfb}"
    for command in xdotool xclip; do
      command -v "$command" >/dev/null || {
        printf 'Required command not found: %s\n' "$command" >&2
        exit 2
      }
    done
    ;;
  wayland)
    : "${WAYLAND_DISPLAY:?Run this probe inside a Wayland session}"
    : "${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR is required for Wayland}"
    : "${SWAYSOCK:?Sway must export its IPC socket for display-size detection}"
    for command in python3 swaymsg wtype wl-paste; do
      command -v "$command" >/dev/null || {
        printf 'Required command not found: %s\n' "$command" >&2
        exit 2
      }
    done
    python3 -c 'from wayland_automation import Mouse' >/dev/null 2>&1 || {
      printf 'Install wayland-automation (see backlog/features/e2e-tests/technical.md)\n' >&2
      exit 2
    }
    ;;
esac

tmp=$(mktemp -d "${TMPDIR:-/tmp}/graphene-gui-probe.XXXXXX")
app_pid=
shell_pid=
passed=0

stop_app() {
  [[ -n "$app_pid" ]] || return 0

  shell_pid=$(pgrep -P "$app_pid" | head -n1 || true)
  if [[ -n "$shell_pid" ]]; then
    kill -TERM "$shell_pid" 2>/dev/null || true
  fi

  for _ in $(seq 1 40); do
    if ! kill -0 "$app_pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
  fi
  wait "$app_pid" 2>/dev/null || true

  if [[ -n "$shell_pid" ]] && kill -0 "$shell_pid" 2>/dev/null; then
    kill -KILL "$shell_pid" 2>/dev/null || true
  fi
  app_pid=
  shell_pid=
}

cleanup() {
  result=$?
  if ((result == 0)); then
    stop_app
    rm -rf -- "$tmp"
  else
    printf 'Probe artifacts kept at %s\n' "$tmp" >&2
    if [[ -f "$tmp/graphene.log" ]]; then
      cat "$tmp/graphene.log" >&2
    fi
    case "$backend" in
      x11)
        if command -v import >/dev/null; then
          import -window root "$tmp/screenshot.png" 2>/dev/null || true
        fi
        ;;
      wayland)
        if command -v grim >/dev/null; then
          grim "$tmp/screenshot.png" 2>/dev/null || true
        fi
        ;;
    esac
    stop_app
  fi
}
trap cleanup EXIT

mkdir -p "$tmp/home/.config/graphene" "$tmp/state"
cat >"$tmp/home/.config/graphene/config.toml" <<'EOF'
[updates]
check = false

[window]
width = 960
height = 600
EOF

cat >"$tmp/shell" <<'EOF'
#!/bin/sh
printf 'GRAPHENE_E2E_READY\n'
export PROMPT_COMMAND=': > "$GRAPHENE_E2E_PROMPT_READY"; unset PROMPT_COMMAND'
exec /bin/bash --noprofile --norc -i
EOF
chmod +x "$tmp/shell"

export HOME="$tmp/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_STATE_HOME="$tmp/state"
export GRAPHENE_E2E_PROMPT_READY="$tmp/prompt-ready"
export SHELL="$tmp/shell"
export GRAPHENE_SHELL_INTEGRATION=0

"$binary" >"$tmp/graphene.log" 2>&1 &
app_pid=$!

wait_for_prompt() {
  for _ in $(seq 1 200); do
    if [[ -f "$GRAPHENE_E2E_PROMPT_READY" ]]; then
      return 0
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      printf 'Graphene exited before its shell prompt was ready\n' >&2
      return 1
    fi
    sleep 0.05
  done
  printf 'Timed out waiting for the controlled shell prompt\n' >&2
  return 1
}

wait_for_prompt

read_clipboard() {
  case "$backend" in
    x11) xclip -selection clipboard -o 2>/dev/null || true ;;
    wayland) wl-paste --primary --no-newline 2>/dev/null || true ;;
  esac
}

if [[ "$backend" == wayland ]]; then
  # A clipboard read of the fixture's startup output proves the native window has
  # rendered and received pointer input before the keyboard probe starts.
  for _ in $(seq 1 30); do
    python3 - 50 <<'PY'
import sys
from wayland_automation import Mouse

y = int(sys.argv[1])
with Mouse() as mouse:
    mouse.swipe(20, y, 600, y, speed=0.1)
PY
    copied=$(read_clipboard)
    if [[ "$copied" == *GRAPHENE_E2E_READY* ]]; then
      break
    fi
    sleep 0.05
  done
  if [[ "$copied" != *GRAPHENE_E2E_READY* ]]; then
    printf 'The Wayland window did not expose the controlled shell output\n' >&2
    exit 1
  fi
fi

# The leading x tolerates the first virtual-key event being consumed while a
# compositor registers its virtual keyboard; the selection starts after the prompt.
marker=E2E_KEYBOARD_MARKER
case "$backend" in
  x11)
    window=$(timeout 15 xdotool search --sync --onlyvisible --classname graphene | head -n1)
    xdotool windowfocus "$window"
    xdotool type --clearmodifiers "xGUI_$marker"
    row_y=50
    ;;
  wayland)
    wtype -- "xGUI_$marker"
    row_y=70
    ;;
esac

case "$backend" in
  x11)
    xdotool mousemove 140 "$row_y" mousedown 1 \
      mousemove --sync 700 "$row_y" mouseup 1
    xdotool key ctrl+shift+c
    ;;
  wayland)
    python3 - "$row_y" <<'PY'
import sys
from wayland_automation import Mouse

y = int(sys.argv[1])
with Mouse() as mouse:
    mouse.swipe(140, y, 700, y, speed=0.1)
PY
    ;;
esac

for _ in $(seq 1 100); do
  copied=$(read_clipboard)
  if [[ "$copied" == *"$marker"* ]]; then
    printf '%s keyboard+pointer behavioral probe passed\n' "$backend"
    passed=1
    break
  fi
  sleep 0.05
done

if ((passed == 0)); then
  printf 'Expected %s in the UI selection; got: %s\n' "$marker" "$copied" >&2
  exit 1
fi

# Stop the real application; its PTY shell is torn down with the window process.
stop_app
printf 'Graphene process stopped\n'
