#!/usr/bin/env bash
set -Eeuo pipefail

backend=${1:-}
scenario=${2:-smoke}
case "$backend" in
  x11|wayland) ;;
  *)
    printf 'Usage: %s <x11|wayland> [scenario]\n' "$0" >&2
    exit 2
    ;;
esac

case "$scenario" in
  smoke|tabs|palette|panes|menu|dialogs|settings|selection|clipboard|scroll|font|mouse|blocks|shell-bash|shell-zsh|shell-fish) ;;
  *)
    printf 'Unknown scenario: %s\n' "$scenario" >&2
    exit 2
    ;;
esac

binary=${GRAPHENE_BIN:-target/debug/graphene}
if [[ ! -x "$binary" ]]; then
  printf 'Graphene executable is missing or not executable: %s\n' "$binary" >&2
  exit 2
fi

for command in awk bash cat chmod find grep head mktemp pgrep ps rm sed seq sha256sum sleep timeout wc; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 2
  }
done

case "$backend" in
  x11)
    : "${DISPLAY:?Run this probe inside an X11 session, e.g. xvfb-run}"
    for command in convert import tesseract xclip xdotool; do
      command -v "$command" >/dev/null || {
        printf 'Required command not found: %s\n' "$command" >&2
        exit 2
      }
    done
    ;;
  wayland)
    for command in convert grim sway swaymsg python3 tesseract wl-copy wl-paste wtype; do
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
  local state
  state=$(ps -o stat= -p "$pid" 2>/dev/null || true)
  [[ -z "$state" || "$state" == Z* ]] && return 0
  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 40); do
    state=$(ps -o stat= -p "$pid" 2>/dev/null || true)
    if [[ -z "$state" || "$state" == Z* ]]; then
      break
    fi
    sleep 0.05
  done
  state=$(ps -o stat= -p "$pid" 2>/dev/null || true)
  if [[ -n "$state" && "$state" != Z* ]]; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
}

stop_app() {
  [[ -n "$app_pid" ]] || return 0

  if [[ -f "$GRAPHENE_E2E_PANE_PIDS" ]]; then
    while read -r _ shell_pid; do
      [[ -n "${shell_pid:-}" ]] || continue
      local children
      children=$(pgrep -P "$shell_pid" || true)
      while read -r child_pid; do
        [[ -n "${child_pid:-}" ]] && stop_pid "$child_pid"
      done <<<"$children"
      stop_pid "$shell_pid"
    done <"$GRAPHENE_E2E_PANE_PIDS"
  else
    shell_pid=$(pgrep -P "$app_pid" | head -n1 || true)
    if [[ -n "$shell_pid" ]]; then
      stop_pid "$shell_pid"
    fi
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
export GRAPHENE_E2E_PROMPT_COUNT="$tmp/prompt-count"
export GRAPHENE_E2E_COMMAND_RESULT_FILE="$tmp/command-result"
export GRAPHENE_E2E_PANE_IDS="$tmp/pane-ids"
export GRAPHENE_E2E_PANE_PIDS="$tmp/pane-pids"
export GRAPHENE_E2E_PANE_SIZES="$tmp/pane-sizes"
export GRAPHENE_E2E_PANE_COUNTER="$tmp/pane-counter"
export GRAPHENE_E2E_ACTIVE_PANES="$tmp/active-panes"
export GRAPHENE_E2E_TEXT="$tmp/text"
export GRAPHENE_E2E_ROWS="$tmp/rows"
export GRAPHENE_E2E_MOUSE_REPORT="$tmp/mouse-report"
export GRAPHENE_E2E_MOUSE_READY="$tmp/mouse-ready"

# Shell-integration scenarios start a real bash, zsh or fish with Graphene's hooks;
# every other scenario uses the controlled fixture below without integration.
case "$scenario" in
  shell-bash|blocks|scroll) integration_shell=bash ;;
  shell-zsh) integration_shell=zsh ;;
  shell-fish) integration_shell=fish ;;
  *) integration_shell= ;;
esac
if [[ -n "$integration_shell" ]]; then
  SHELL=$(command -v "$integration_shell") || {
    printf 'Required command not found: %s\n' "$integration_shell" >&2
    exit 2
  }
  export SHELL
  unset GRAPHENE_SHELL_INTEGRATION
  # The only shell in these scenarios is pane 1. The user startup files, written in the
  # disposable home, give a fixed prompt and the same readiness markers as the fixture;
  # Graphene's integration loads them after installing its own hooks.
  export GRAPHENE_E2E_PANE_ID=1
  printf '1\n' >"$GRAPHENE_E2E_PANE_COUNTER"
  printf '1\n' >"$GRAPHENE_E2E_PANE_IDS"
  cat >"$HOME/.bashrc" <<'EOF'
printf '1 %s\n' "$$" >> "$GRAPHENE_E2E_PANE_PIDS"
PS1='$ '
PROMPT_COMMAND=': > "$GRAPHENE_E2E_PROMPT_READY"; printf "1\n" >> "$GRAPHENE_E2E_PROMPT_COUNT"'
EOF
  cat >"$HOME/.zshrc" <<'EOF'
printf '1 %s\n' "$$" >> "$GRAPHENE_E2E_PANE_PIDS"
PROMPT='$ '
_e2e_precmd() { : > "$GRAPHENE_E2E_PROMPT_READY"; printf '1\n' >> "$GRAPHENE_E2E_PROMPT_COUNT"; }
precmd_functions+=(_e2e_precmd)
EOF
  mkdir -p "$XDG_CONFIG_HOME/fish"
  cat >"$XDG_CONFIG_HOME/fish/config.fish" <<'EOF'
printf '1 %s\n' $fish_pid >> "$GRAPHENE_E2E_PANE_PIDS"
set -g fish_greeting
function fish_prompt
    printf '$ '
end
function _e2e_prompt --on-event fish_prompt
    printf '' > "$GRAPHENE_E2E_PROMPT_READY"
    printf '1\n' >> "$GRAPHENE_E2E_PROMPT_COUNT"
end
EOF
else
  export SHELL="$tmp/shell"
  export GRAPHENE_SHELL_INTEGRATION=0
fi

cat >"$tmp/config/graphene/config.toml" <<'EOF'
language = "en"

[updates]
check = false

[window]
width = 960
height = 600
EOF

cat >"$tmp/shell" <<'EOF'
#!/bin/sh
pane_id=$(cat "$GRAPHENE_E2E_PANE_COUNTER" 2>/dev/null || printf '0')
pane_id=$((pane_id + 1))
printf '%s\n' "$pane_id" > "$GRAPHENE_E2E_PANE_COUNTER"
printf '%s\n' "$pane_id" >> "$GRAPHENE_E2E_PANE_IDS"
printf '%s %s\n' "$pane_id" "$$" >> "$GRAPHENE_E2E_PANE_PIDS"
export GRAPHENE_E2E_PANE_ID=$pane_id
printf 'GRAPHENE_E2E_READY\n'
export PS1=
export PROMPT_COMMAND=': > "$GRAPHENE_E2E_PROMPT_READY"; printf "%s\n" "$GRAPHENE_E2E_PANE_ID" >> "$GRAPHENE_E2E_PROMPT_COUNT"'
exec /bin/bash --noprofile --norc -i
EOF
chmod +x "$tmp/shell"

# Text shown by the terminal scenarios: written by the probe, so the words located on
# screen never appear in a command line typed on the same screen.
printf 'ALPHA BRAVO CHARLIE\nDELTA ECHO FOXTROT\n' >"$GRAPHENE_E2E_TEXT"
seq -f 'ROW %03g' 1 300 >"$GRAPHENE_E2E_ROWS"

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
default_border none
default_floating_border none
output HEADLESS-1 mode 1280x800
for_window [title="Settings"] floating enable, border none, resize set 1000 700, move position 0 0
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
prompt_count() {
  [[ -f "$GRAPHENE_E2E_PROMPT_COUNT" ]] && wc -l <"$GRAPHENE_E2E_PROMPT_COUNT" || printf '0\n'
}

wait_for_prompt_count() {
  local expected=$1
  local description=$2
  for _ in $(seq 1 200); do
    if (( $(prompt_count) >= expected )); then
      return 0
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      printf 'Graphene exited before %s\n' "$description" >&2
      return 1
    fi
    sleep 0.05
  done
  printf 'Timed out waiting for %s (expected prompt count %s)\n' "$description" "$expected" >&2
  return 1
}

wait_for_line_count() {
  local file=$1
  local expected=$2
  local description=$3
  for _ in $(seq 1 200); do
    local actual=0
    [[ -f "$file" ]] && actual=$(wc -l <"$file")
    if ((actual >= expected)); then
      if [[ "$file" != "$GRAPHENE_E2E_PANE_IDS" ]]; then
        return 0
      fi
      [[ -f "$GRAPHENE_E2E_PROMPT_COUNT" ]] || {
        sleep 0.05
        continue
      }
      local ready=true
      for pane_id in $(seq 1 "$expected"); do
        grep -Fxq "$pane_id" "$GRAPHENE_E2E_PROMPT_COUNT" || { ready=false; break; }
      done
      [[ "$ready" == true ]] && return 0
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      printf 'Graphene exited before %s\n' "$description" >&2
      return 1
    fi
    sleep 0.05
  done
  printf 'Timed out waiting for %s (expected %s lines)\n' "$description" "$expected" >&2
  return 1
}

wtype_settle_ms=${GRAPHENE_E2E_WTYPE_SETTLE_MS:-500}

send_key() {
  local chord=$1
  if [[ "$backend" == x11 ]]; then
    local -a x11_parts
    IFS=+ read -r -a x11_parts <<<"$chord"
    case "${x11_parts[-1]}" in
      enter) x11_parts[-1]=Return ;;
      esc) x11_parts[-1]=Escape ;;
      up) x11_parts[-1]=Up ;;
      down) x11_parts[-1]=Down ;;
      left) x11_parts[-1]=Left ;;
      right) x11_parts[-1]=Right ;;
      tab) x11_parts[-1]=Tab ;;
      pageup) x11_parts[-1]=Page_Up ;;
      pagedown) x11_parts[-1]=Page_Down ;;
      insert) x11_parts[-1]=Insert ;;
      home) x11_parts[-1]=Home ;;
      end) x11_parts[-1]=End ;;
    esac
    local normalized
    normalized=$(IFS=+; printf '%s' "${x11_parts[*]}")
    xdotool key --clearmodifiers "$normalized"
    return
  fi

  # Each wtype process creates a new virtual keyboard; its initial sleep lets Graphene
  # receive the new keymap before the first key, which would otherwise be dropped.
  local -a args=(-s "$wtype_settle_ms")
  # Graphene matches Alt+digit by its physical key, but wtype numbers its keycodes in
  # the order keysyms first appear. Releasing N unused keys first (a release without a
  # press has no effect) moves digit N onto evdev KEY_N's code.
  if [[ "$chord" =~ ^alt\+([1-9])$ ]]; then
    local filler
    for ((filler = 0; filler < BASH_REMATCH[1]; filler++)); do
      args+=(-p "F$((13 + filler))")
    done
  fi
  local -a modifiers=()
  local -a parts
  IFS=+ read -r -a parts <<<"$chord"
  local part
  for part in "${parts[@]:0:${#parts[@]}-1}"; do
    case "$part" in
      ctrl|shift|alt)
        modifiers+=("$part")
        args+=(-M "$part")
        ;;
      *) printf 'Unsupported Wayland modifier: %s\n' "$part" >&2; return 2 ;;
    esac
  done
  part=${parts[-1]}
  case "$part" in
    enter) part=Return ;;
    esc) part=Escape ;;
    up) part=Up ;;
    down) part=Down ;;
    left) part=Left ;;
    right) part=Right ;;
    tab) part=Tab ;;
    pageup) part=Page_Up ;;
    pagedown) part=Page_Down ;;
    insert) part=Insert ;;
    home) part=Home ;;
    end) part=End ;;
    comma) part=comma ;;
    *) ;;
  esac
  args+=(-P "$part" -p "$part")
  for ((modifier_index = ${#modifiers[@]} - 1; modifier_index >= 0; modifier_index--)); do
    args+=(-m "${modifiers[modifier_index]}")
  done
  wtype "${args[@]}"
}

type_text() {
  if [[ "$backend" == x11 ]]; then
    xdotool type --clearmodifiers --delay 1 -- "$1"
  else
    wtype -s "$wtype_settle_ms" -d 5 "$1"
  fi
}

click_pointer() {
  local x=$1 y=$2 button=$3
  if [[ "$backend" == x11 ]]; then
    local button_number=1
    [[ "$button" == right ]] && button_number=3
    [[ "$button" == middle ]] && button_number=2
    xdotool mousemove --sync "$x" "$y" click --clearmodifiers "$button_number"
  else
    python3 - "$x" "$y" "$button" <<'PY'
import sys
from wayland_automation import Mouse

with Mouse() as mouse:
    mouse.click(int(sys.argv[1]), int(sys.argv[2]), sys.argv[3])
PY
  fi
}

record_active_pane() {
  local expected=$1
  local before=0
  local prompts_before
  prompts_before=$(prompt_count)
  [[ -f "$GRAPHENE_E2E_ACTIVE_PANES" ]] && before=$(wc -l <"$GRAPHENE_E2E_ACTIVE_PANES")
  type_text 'printf "%s\n" "$GRAPHENE_E2E_PANE_ID" >> "$GRAPHENE_E2E_ACTIVE_PANES"'
  send_key enter
  wait_for_line_count "$GRAPHENE_E2E_ACTIVE_PANES" "$((before + 1))" 'the active pane marker'
  wait_for_prompt_count "$((prompts_before + 1))" 'the active-pane command prompt'
  local actual
  actual=$(sed -n "$((before + 1))p" "$GRAPHENE_E2E_ACTIVE_PANES")
  [[ "$actual" == "$expected" ]] || {
    printf 'Expected active pane %s; got %s\n' "$expected" "$actual" >&2
    return 1
  }
}

measure_active_pane() {
  local expected=$1
  local before=0
  local prompts_before
  prompts_before=$(prompt_count)
  [[ -f "$GRAPHENE_E2E_PANE_SIZES" ]] && before=$(wc -l <"$GRAPHENE_E2E_PANE_SIZES")
  type_text 'printf "%s " "$GRAPHENE_E2E_PANE_ID" >> "$GRAPHENE_E2E_PANE_SIZES"; stty size >> "$GRAPHENE_E2E_PANE_SIZES"'
  send_key enter
  wait_for_line_count "$GRAPHENE_E2E_PANE_SIZES" "$((before + 1))" 'the active pane terminal size'
  wait_for_prompt_count "$((prompts_before + 1))" 'the terminal-size command prompt'
  local pane rows columns
  read -r pane rows columns <<<"$(sed -n "$((before + 1))p" "$GRAPHENE_E2E_PANE_SIZES")"
  [[ "$pane" == "$expected" ]] || {
    printf 'Expected pane %s while reading terminal size; got pane %s\n' "$expected" "$pane" >&2
    return 1
  }
  printf '%s %s\n' "$rows" "$columns"
}

wait_for_columns_greater() {
  local pane=$1 baseline=$2
  for _ in $(seq 1 40); do
    local size rows columns
    size=$(measure_active_pane "$pane")
    read -r rows columns <<<"$size"
    ((columns > baseline)) && {
      printf '%s %s\n' "$rows" "$columns"
      return 0
    }
    sleep 0.05
  done
  printf 'Pane %s columns did not grow beyond %s\n' "$pane" "$baseline" >&2
  return 1
}

wait_for_columns_less() {
  local pane=$1 baseline=$2
  for _ in $(seq 1 40); do
    local size rows columns
    size=$(measure_active_pane "$pane")
    read -r rows columns <<<"$size"
    ((columns < baseline)) && {
      printf '%s %s\n' "$rows" "$columns"
      return 0
    }
    sleep 0.05
  done
  printf 'Pane %s columns did not shrink below %s\n' "$pane" "$baseline" >&2
  return 1
}

wait_for_columns_equal_to() {
  local pane=$1 expected=$2
  for _ in $(seq 1 40); do
    local size rows columns
    size=$(measure_active_pane "$pane")
    read -r rows columns <<<"$size"
    ((columns == expected)) && {
      printf '%s %s\n' "$rows" "$columns"
      return 0
    }
    sleep 0.05
  done
  printf 'Pane %s width did not return to %s columns\n' "$pane" "$expected" >&2
  return 1
}

wait_for_rows_greater() {
  local pane=$1 baseline=$2
  for _ in $(seq 1 40); do
    local size rows columns
    size=$(measure_active_pane "$pane")
    read -r rows columns <<<"$size"
    ((rows > baseline)) && {
      printf '%s %s\n' "$rows" "$columns"
      return 0
    }
    sleep 0.05
  done
  printf 'Pane %s rows did not grow beyond %s\n' "$pane" "$baseline" >&2
  return 1
}

wait_for_rows_less() {
  local pane=$1 baseline=$2
  for _ in $(seq 1 40); do
    local size rows columns
    size=$(measure_active_pane "$pane")
    read -r rows columns <<<"$size"
    ((rows < baseline)) && {
      printf '%s %s\n' "$rows" "$columns"
      return 0
    }
    sleep 0.05
  done
  printf 'Pane %s rows did not shrink below %s\n' "$pane" "$baseline" >&2
  return 1
}

wait_for_columns_equal() {
  local pane=$1 direction=$2 other=$3 return_direction=$4
  for _ in $(seq 1 40); do
    local size rows columns other_size other_rows other_columns
    size=$(measure_active_pane "$pane")
    read -r rows columns <<<"$size"
    send_key "$direction"
    other_size=$(measure_active_pane "$other")
    read -r other_rows other_columns <<<"$other_size"
    send_key "$return_direction"
    if ((columns - other_columns >= -1 && columns - other_columns <= 1)); then
      printf '%s %s\n' "$rows" "$columns"
      return 0
    fi
    sleep 0.05
  done
  printf 'Pane widths did not equalise (pane %s: %s; pane %s: %s)\n' \
    "$pane" "$columns" "$other" "$other_columns" >&2
  return 1
}

wait_for_foreground_sleep() {
  local pane_id=$1
  local shell_pid
  shell_pid=$(awk -v id="$pane_id" '$1 == id { print $2 }' "$GRAPHENE_E2E_PANE_PIDS")
  for _ in $(seq 1 200); do
    pgrep -P "$shell_pid" -x sleep >/dev/null && return 0
    sleep 0.05
  done
  printf 'Pane %s did not start the foreground sleep process\n' "$pane_id" >&2
  return 1
}

# Graphene draws light text on a dark background at UI size; inverting and enlarging
# the capture makes Tesseract read it reliably. TSV coordinates are therefore doubled.
ocr_text() {
  convert "$1" -colorspace Gray -negate -resize 200% "$1.ocr.png"
  tesseract "$1.ocr.png" stdout -l eng "${@:2}" 2>/dev/null
}

capture_region() {
  local x=$1 y=$2 width=$3 height=$4 file=$5
  if [[ "$backend" == x11 ]]; then
    import -window root -crop "${width}x${height}+${x}+${y}" +repage "$file"
  else
    grim -g "$x,$y ${width}x${height}" "$file"
  fi
}

# OCR text of the main window.
window_text() {
  local x y width height
  read -r x y width height <<<"$(main_window_rect)"
  capture_region "$x" "$y" "$width" "$height" "$tmp/window.png"
  ocr_text "$tmp/window.png" --psm 11
}

wait_for_window_text() {
  local pattern=$1 description=$2
  for _ in $(seq 1 60); do
    window_text | grep -Eq -- "$pattern" && return 0
    sleep 0.05
  done
  printf 'Could not read %s (/%s/) in the window\n' "$description" "$pattern" >&2
  return 1
}

wait_for_window_text_gone() {
  local pattern=$1 description=$2
  for _ in $(seq 1 60); do
    window_text | grep -Eq -- "$pattern" || return 0
    sleep 0.05
  done
  printf '%s (/%s/) was still visible in the window\n' "$description" "$pattern" >&2
  return 1
}

# Screen box `x y width height` of the first word of the main window read as exactly
# `word`, top to bottom then left to right.
locate_word() {
  local word=$1
  local x y width height box
  read -r x y width height <<<"$(main_window_rect)"
  for _ in $(seq 1 60); do
    capture_region "$x" "$y" "$width" "$height" "$tmp/window.png"
    if box=$(ocr_text "$tmp/window.png" --psm 11 tsv | python3 -c '
import sys

word = sys.argv[1]
found = []
for row in sys.stdin.read().splitlines()[1:]:
    columns = row.split("\t")
    if len(columns) == 12 and columns[11].strip() == word:
        left, top, width, height = (int(value) // 2 for value in columns[6:10])
        found.append((top, left, width, height))
if not found:
    raise SystemExit(1)
top, left, width, height = min(found)
print(left, top, width, height)
' "$word"); then
      local left top box_width box_height
      read -r left top box_width box_height <<<"$box"
      printf '%s %s %s %s\n' "$((x + left))" "$((y + top))" "$box_width" "$box_height"
      return 0
    fi
    sleep 0.05
  done
  printf 'Could not locate the word %s in the window\n' "$word" >&2
  return 1
}

# Centre of character `index` (from 0) of a located word of `length` characters.
char_x() {
  local left=$1 width=$2 length=$3 index=$4
  printf '%s\n' "$((left + (2 * index + 1) * width / (2 * length)))"
}

# A selection includes its first and last cells only from the side facing the rest of
# the selection: drags start in the left quarter and end in the right quarter of a cell.
char_start_x() {
  local left=$1 width=$2 length=$3 index=$4
  printf '%s\n' "$((left + (4 * index + 1) * width / (4 * length)))"
}

char_end_x() {
  local left=$1 width=$2 length=$3 index=$4
  printf '%s\n' "$((left + (4 * index + 3) * width / (4 * length)))"
}

# Pointer input. `pointer_clicks x y count [button] [modifier]` clicks `count` times in a
# row; `pointer_drag x1 y1 x2 y2 [modifier]` drags with the left button. A modifier is
# held around the gesture: with xdotool on X11, with a wtype keyboard that holds it
# while the virtual pointer acts on Wayland.
wayland_pointer() {
  python3 - "$@" <<'PY'
import sys
from wayland_automation import Mouse

with Mouse() as mouse:
    for operation in sys.argv[1:]:
        name, *args = operation.split()
        if name == "click":
            mouse.click(int(args[0]), int(args[1]), args[2])
        elif name == "move":
            mouse.click(int(args[0]), int(args[1]), "nothing")
        elif name == "down":
            mouse.mouse_down(int(args[0]), int(args[1]), args[2])
        elif name == "up":
            mouse.mouse_up(int(args[0]), int(args[1]), args[2])
        elif name == "scroll":
            mouse.scroll(int(args[0]), int(args[1]), int(args[2]))
PY
}

with_wayland_modifier() {
  local modifier=$1
  shift
  if [[ -z "$modifier" ]]; then
    "$@"
    return
  fi
  # The modifier is held for longer than the gesture takes; the gesture starts once the
  # new virtual keyboard has delivered its keymap and the modifier press.
  wtype -s "$wtype_settle_ms" -M "$modifier" -s 3000 -m "$modifier" &
  local holder=$!
  sleep "$(awk -v ms="$wtype_settle_ms" 'BEGIN { printf "%.3f", (ms + 400) / 1000 }')"
  "$@"
  wait "$holder"
}

pointer_clicks() {
  local x=$1 y=$2 count=$3 button=${4:-left} modifier=${5:-}
  if [[ "$backend" == x11 ]]; then
    local button_number=1
    [[ "$button" == middle ]] && button_number=2
    [[ "$button" == right ]] && button_number=3
    local -a command=(xdotool)
    [[ -n "$modifier" ]] && command+=(keydown "$modifier")
    command+=(mousemove --sync "$x" "$y" click --repeat "$count" --delay 60 "$button_number")
    [[ -n "$modifier" ]] && command+=(keyup "$modifier")
    "${command[@]}"
  else
    local -a operations=()
    for ((click = 0; click < count; click++)); do
      operations+=("click $x $y $button")
    done
    with_wayland_modifier "$modifier" wayland_pointer "${operations[@]}"
  fi
}

pointer_drag() {
  local x1=$1 y1=$2 x2=$3 y2=$4 modifier=${5:-}
  local middle_x=$(((x1 + x2) / 2)) middle_y=$(((y1 + y2) / 2))
  if [[ "$backend" == x11 ]]; then
    local -a command=(xdotool)
    [[ -n "$modifier" ]] && command+=(keydown "$modifier")
    command+=(mousemove --sync "$x1" "$y1" mousedown 1
      mousemove --sync "$middle_x" "$middle_y" mousemove --sync "$x2" "$y2" mouseup 1)
    [[ -n "$modifier" ]] && command+=(keyup "$modifier")
    "${command[@]}"
  else
    with_wayland_modifier "$modifier" wayland_pointer "move $x1 $y1" "down $x1 $y1 left" \
      "move $middle_x $middle_y" "move $x2 $y2" "up $x2 $y2 left"
  fi
}

pointer_scroll() {
  local x=$1 y=$2 clicks=$3
  if [[ "$backend" == x11 ]]; then
    local button=4
    ((clicks < 0)) && button=5
    xdotool mousemove --sync "$x" "$y" click --repeat "${clicks#-}" --delay 30 "$button"
  else
    wayland_pointer "scroll $clicks $x $y"
  fi
}

# Clipboard access from outside Graphene: `clipboard` is the regular clipboard,
# `primary` the selection a mouse selection fills and a middle click pastes.
read_clipboard() {
  local kind=$1
  if [[ "$backend" == x11 ]]; then
    xclip -selection "$kind" -o 2>/dev/null || true
  elif [[ "$kind" == primary ]]; then
    wl-paste --primary --no-newline 2>/dev/null || true
  else
    wl-paste --no-newline 2>/dev/null || true
  fi
}

set_clipboard() {
  local kind=$1 text=$2
  if [[ "$backend" == x11 ]]; then
    printf '%s' "$text" | xclip -selection "$kind" -i >/dev/null 2>&1
  elif [[ "$kind" == primary ]]; then
    printf '%s' "$text" | wl-copy --primary >/dev/null 2>&1
  else
    printf '%s' "$text" | wl-copy >/dev/null 2>&1
  fi
  for _ in $(seq 1 100); do
    [[ "$(read_clipboard "$kind")" == "$text" ]] && return 0
    sleep 0.05
  done
  printf 'Could not set the %s selection\n' "$kind" >&2
  return 1
}

wait_for_clipboard() {
  local kind=$1 expected=$2 description=$3 actual=
  for _ in $(seq 1 100); do
    actual=$(read_clipboard "$kind")
    [[ "$actual" == "$expected" ]] && return 0
    sleep 0.05
  done
  printf 'Expected %s in the %s selection after %s; got %q\n' \
    "$(printf '%q' "$expected")" "$kind" "$description" "$actual" >&2
  return 1
}

# Sends `chord` with a sentinel in the clipboard and waits for Graphene to copy
# `expected` there.
expect_copy() {
  local chord=$1 expected=$2 description=$3
  set_clipboard clipboard "E2E_SENTINEL_$RANDOM"
  send_key "$chord"
  wait_for_clipboard clipboard "$expected" "$description"
}

# Text the primary selection holds after `gesture` (a pointer command and its
# arguments), once Graphene has replaced the sentinel put there first.
selection_after() {
  local sentinel="E2E_SENTINEL_$RANDOM" actual=
  set_clipboard primary "$sentinel"
  "$@"
  for _ in $(seq 1 100); do
    actual=$(read_clipboard primary)
    if [[ "$actual" != "$sentinel" ]]; then
      printf '%s\n' "$actual"
      return 0
    fi
    sleep 0.05
  done
  printf 'The pointer gesture "%s" selected nothing\n' "$*" >&2
  return 1
}

expect_selection() {
  local expected=$1 description=$2
  shift 2
  local actual
  actual=$(selection_after "$@")
  [[ "$actual" == "$expected" ]] || {
    printf 'Expected %q to be selected by %s; got %q\n' "$expected" "$description" "$actual" >&2
    return 1
  }
}

run_shell_command() {
  local prompts_before
  prompts_before=$(prompt_count)
  type_text "$1"
  send_key enter
  wait_for_prompt_count "$((prompts_before + 1))" "the prompt after: $1"
}

wait_for_command_result() {
  local expected=$1 description=$2 result=
  for _ in $(seq 1 200); do
    [[ -f "$GRAPHENE_E2E_COMMAND_RESULT_FILE" ]] && result=$(<"$GRAPHENE_E2E_COMMAND_RESULT_FILE")
    [[ "$result" == "$expected" ]] && return 0
    sleep 0.05
  done
  printf 'Expected %s to write %q; got %q\n' "$description" "$expected" "$result" >&2
  return 1
}

# Clears the screen and shows the fixture text on its first two rows.
show_fixture_text() {
  run_shell_command 'clear; cat "$GRAPHENE_E2E_TEXT"'
}

wait_for_confirmation_dialog() {
  local expected=$1
  # The dialog is centred on the main window. Tesseract's page segmentation misses it
  # in a full-screen capture, so only the central half of the window is read.
  local x y width height
  read -r x y width height <<<"$(main_window_rect)"
  local region="$((width / 2))x$((height / 2))+$((x + width / 4))+$((y + height / 4))"
  for _ in $(seq 1 40); do
    if [[ "$backend" == x11 ]]; then
      import -window root -crop "$region" +repage "$tmp/dialog.png"
    else
      grim -g "$((x + width / 4)),$((y + height / 4)) $((width / 2))x$((height / 2))" "$tmp/dialog.png"
    fi
    if ocr_text "$tmp/dialog.png" | grep -Fqi "$expected"; then
      return 0
    fi
    sleep 0.05
  done
  printf 'Could not observe close confirmation text: %s\n' "$expected" >&2
  return 1
}

run_palette_action() {
  local query=$1 action_id=$2
  send_key ctrl+shift+p
  type_text "$query"
  send_key enter
  for _ in $(seq 1 200); do
    if [[ -f "$XDG_STATE_HOME/graphene/palette-recent" ]] &&
      [[ $(head -n1 "$XDG_STATE_HOME/graphene/palette-recent") == "$action_id" ]]; then
      return 0
    fi
    sleep 0.05
  done
  printf 'Palette did not run action %s for query %s\n' "$action_id" "$query" >&2
  return 1
}

choose_palette_item() {
  send_key ctrl+shift+p
  type_text "$1"
  send_key enter
}

main_window_rect() {
  if [[ "$backend" == x11 ]]; then
    xdotool getwindowgeometry --shell "$window" |
      awk -F= '/^(X|Y|WIDTH|HEIGHT)=/ { printf "%s ", $2 }'
  else
    swaymsg -t get_tree | python3 -c '
import json, sys
def walk(node):
    if node.get("focused") and node.get("type") == "con":
        r = node["rect"]
        print(r["x"], r["y"], r["width"], r["height"])
        return True
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        if walk(child):
            return True
    return False
walk(json.load(sys.stdin))
'
  fi
}

click_pane_half() {
  local half=$1
  local x y width height target_x
  read -r x y width height <<<"$(main_window_rect)"
  if [[ "$half" == left ]]; then
    target_x=$((x + width / 4))
  else
    target_x=$((x + 3 * width / 4))
  fi
  click_pointer "$target_x" "$((y + height / 2))" left
}

drag_divider_left() {
  local x y width height start_x end_x center_y
  read -r x y width height <<<"$(main_window_rect)"
  start_x=$((x + width / 2))
  end_x=$((start_x - 40))
  center_y=$((y + height / 2))
  if [[ "$backend" == x11 ]]; then
    xdotool mousemove --sync "$start_x" "$center_y" mousedown 1 \
      mousemove --sync "$end_x" "$center_y" mouseup 1
  else
    python3 - "$start_x" "$center_y" "$end_x" <<'PY'
import sys
from wayland_automation import Mouse

start_x, y, end_x = map(int, sys.argv[1:])
with Mouse() as mouse:
    mouse.mouse_down(start_x, y)
    mouse.click(end_x, y, "nothing")
    mouse.mouse_up()
PY
  fi
}

click_palette_first_match() {
  local query=$1
  send_key ctrl+shift+p
  type_text "$query"
  local x y width height
  read -r x y width height <<<"$(main_window_rect)"
  local row_x=$((x + width / 2))
  local row_y
  row_y=$(awk -v y="$y" -v height="$height" 'BEGIN { printf "%d", y + height * 0.15 + 55 }')
  click_pointer "$row_x" "$row_y" left
}

settings_window_count() {
  if [[ "$backend" == x11 ]]; then
    xdotool search --onlyvisible --name '^Settings$' 2>/dev/null | wc -l
  else
    swaymsg -t get_tree | python3 -c '
import json, sys
def walk(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from walk(child)
print(sum(w.get("name") == "Settings" for w in walk(json.load(sys.stdin))))
'
  fi
}

focus_main_window() {
  if [[ "$backend" == x11 ]]; then
    xdotool windowfocus --sync "$window"
  else
    swaymsg "[con_id=$main_sway_con_id] focus" >/dev/null
  fi
}

focus_settings_window() {
  if [[ "$backend" == x11 ]]; then
    local settings
    settings=$(timeout 15 xdotool search --sync --onlyvisible --name '^Settings$' | head -n1)
    xdotool windowfocus --sync "$settings"
  else
    swaymsg '[title="^Settings$"] focus' >/dev/null
  fi
}

wait_for_settings_closed() {
  for _ in $(seq 1 200); do
    [[ $(settings_window_count) == 0 ]] && return 0
    sleep 0.05
  done
  printf 'Escape did not close the Settings window\n' >&2
  return 1
}

wait_for_settings_open() {
  for _ in $(seq 1 200); do
    if [[ "$backend" == x11 ]]; then
      if timeout 1 xdotool search --onlyvisible --name 'Settings' >/dev/null 2>&1; then
        return 0
      fi
    elif swaymsg -t get_tree | python3 -c '
import json, sys
def walk(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from walk(child)
tree = json.load(sys.stdin)
raise SystemExit(0 if any(w.get("name") == "Settings" and w.get("focused") for w in walk(tree)) else 1)
'; then
      return 0
    fi
    sleep 0.05
  done
  printf 'Settings did not open and receive focus\n' >&2
  return 1
}

focus_wayland_window() {
  main_sway_con_id=
  for _ in $(seq 1 200); do
    main_sway_con_id=$(swaymsg -t get_tree | python3 -c '
import json, sys
def walk(node):
    if node.get("type") in ("con", "floating_con") and node.get("name") != "Settings":
        print(node["id"])
        return True
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        if walk(child):
            return True
    return False
walk(json.load(sys.stdin))
')
    [[ -n "$main_sway_con_id" ]] && break
    sleep 0.05
  done
  [[ -n "$main_sway_con_id" ]] || {
    printf 'Could not find the Graphene window in the Sway tree\n' >&2
    return 1
  }
  swaymsg "[con_id=$main_sway_con_id] focus" >/dev/null
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

case "$scenario" in
  smoke)
    # The command is typed through the GUI driver, so the result proves the keyboard
    # input reached the shell through Graphene's window and PTY. The arithmetic makes
    # the expected text absent from the typed characters.
    type_text "printf 'E2E_RESULT_%s\\n' \$((6 * 7)) >\"\$GRAPHENE_E2E_COMMAND_RESULT_FILE\""
    send_key enter
    wait_for_file "$GRAPHENE_E2E_COMMAND_RESULT_FILE" 'the typed command to run'
    expected=E2E_RESULT_42
    result=
    for _ in $(seq 1 100); do
      result=$(<"$GRAPHENE_E2E_COMMAND_RESULT_FILE")
      [[ "$result" == "$expected" ]] && break
      sleep 0.05
    done
    [[ "$result" == "$expected" ]] || {
      printf 'Expected command result %s; got: %s\n' "$expected" "$result" >&2
      exit 1
    }
    printf '%s GUI smoke test passed: typed command produced %s\n' "$backend" "$result"
    ;;
  tabs)
    send_key ctrl+shift+t
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 2 'the second tab shell'
    record_active_pane 2
    send_key ctrl+tab
    record_active_pane 1
    send_key ctrl+shift+tab
    record_active_pane 2
    send_key alt+1
    record_active_pane 1
    send_key alt+2
    record_active_pane 2
    click_pointer 100 16 left
    record_active_pane 1
    click_pointer 456 16 left
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 3 'the tab-bar plus button to open a tab'
    record_active_pane 3
    click_pointer 330 16 middle
    click_pointer 100 16 right
    send_key down
    send_key enter
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 4 'the tab context-menu New tab action'
    record_active_pane 4
    click_pointer 100 16 right
    send_key down
    send_key down
    send_key enter
    record_active_pane 4
    click_pointer 100 16 left
    record_active_pane 3
    send_key ctrl+alt+w
    record_active_pane 4
    printf '%s tab navigation passed: shortcuts, numbered selection, clicks, plus and tab context menu\n' "$backend"
    ;;
  palette)
    click_palette_first_match 'new tab'
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 2 'the palette New tab action'
    record_active_pane 2
    [[ $(head -n1 "$XDG_STATE_HOME/graphene/palette-recent") == new_tab ]]
    send_key ctrl+shift+e
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 3 'the split shortcut'
    record_active_pane 3
    run_palette_action 'equalise' equalise_panes
    run_palette_action 'move pane' detach_pane
    record_active_pane 3
    [[ $(wc -l <"$GRAPHENE_E2E_PANE_IDS") == 3 ]] || {
      printf 'Detaching a pane unexpectedly started another shell\n' >&2
      exit 1
    }
    choose_palette_item 'Tab 1'
    record_active_pane 1
    choose_palette_item 'Tab 2'
    record_active_pane 2
    choose_palette_item 'Tab 3'
    record_active_pane 3
    printf '%s palette search, pointer selection, actions and detached tab selection passed\n' "$backend"
    ;;
  panes)
    send_key ctrl+shift+e
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 2 'the split-right shortcut'
    record_active_pane 2
    size=$(measure_active_pane 2)
    read -r before_drag_rows before_drag_columns <<<"$size"
    send_key ctrl+shift+left
    size=$(measure_active_pane 1)
    read -r before_drag_left_rows before_drag_left_columns <<<"$size"
    send_key ctrl+shift+right
    drag_divider_left
    size=$(wait_for_columns_greater 2 "$before_drag_columns")
    read -r drag_rows drag_columns <<<"$size"
    send_key ctrl+shift+left
    size=$(wait_for_columns_less 1 "$before_drag_left_columns")
    read -r drag_left_rows drag_left_columns <<<"$size"
    send_key ctrl+shift+right
    click_pane_half left
    record_active_pane 1
    click_pane_half right
    record_active_pane 2
    send_key ctrl+shift+left
    record_active_pane 1
    send_key ctrl+shift+right
    record_active_pane 2
    send_key ctrl+shift+b
    record_active_pane 1
    send_key ctrl+shift+n
    record_active_pane 2
    size=$(measure_active_pane 2)
    read -r pane2_rows pane2_columns <<<"$size"
    send_key ctrl+shift+left
    size=$(measure_active_pane 1)
    read -r pane1_rows pane1_columns <<<"$size"
    send_key ctrl+shift+right
    send_key ctrl+shift+alt+left
    size=$(wait_for_columns_greater 2 "$pane2_columns")
    read -r resized_rows resized_columns <<<"$size"
    send_key ctrl+shift+left
    size=$(wait_for_columns_less 1 "$pane1_columns")
    read -r shrunk_rows shrunk_columns <<<"$size"
    ((resized_columns - shrunk_columns > 1)) || {
      printf 'Resize fixture did not produce unequal pane widths (%s, %s)\n' \
        "$shrunk_columns" "$resized_columns" >&2
      exit 1
    }
    send_key ctrl+shift+right
    send_key ctrl+shift+z
    size=$(wait_for_columns_greater 2 "$resized_columns")
    read -r zoomed_rows zoomed_columns <<<"$size"
    ((zoomed_columns > resized_columns)) || {
      printf 'Zoom did not increase the active pane width\n' >&2
      exit 1
    }
    send_key ctrl+shift+z
    size=$(wait_for_columns_equal_to 2 "$resized_columns")
    read -r restored_rows restored_columns <<<"$size"
    send_key ctrl+shift+alt+right
    size=$(wait_for_columns_less 2 "$restored_columns")
    read -r narrowed_rows narrowed_columns <<<"$size"
    run_palette_action 'equalise' equalise_panes
    size=$(wait_for_columns_equal 2 ctrl+shift+left 1 ctrl+shift+right)
    read -r equalised_rows equalised_columns <<<"$size"
    run_palette_action 'close pane' close_pane
    record_active_pane 1
    send_key ctrl+shift+o
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 3 'the split-down shortcut'
    size=$(measure_active_pane 3)
    read -r pane3_rows pane3_columns <<<"$size"
    send_key ctrl+shift+up
    size=$(measure_active_pane 1)
    read -r pane1_rows_before pane1_columns_before <<<"$size"
    send_key ctrl+shift+down
    send_key ctrl+shift+alt+up
    size=$(wait_for_rows_greater 3 "$pane3_rows")
    read -r resized3_rows resized3_columns <<<"$size"
    send_key ctrl+shift+up
    size=$(wait_for_rows_less 1 "$pane1_rows_before")
    read -r pane1_rows_after pane1_columns_after <<<"$size"
    send_key ctrl+shift+down
    send_key ctrl+shift+alt+down
    size=$(wait_for_rows_less 3 "$resized3_rows")
    read -r lowered3_rows lowered3_columns <<<"$size"
    record_active_pane 3
    printf '%s pane scenarios passed: resize/zoom dimensions, equalised widths and focus-preserving actions\n' "$backend"
    ;;
  menu)
    click_pointer 120 120 right
    send_key down
    send_key down
    send_key enter
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 2 'the context-menu split-right action'
    record_active_pane 2
    printf '%s pane context menu passed: right-click, keyboard selection and split\n' "$backend"
    ;;
  dialogs)
    send_key ctrl+shift+e
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 2 'the pane-close dialog fixture'
    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 2
    send_key ctrl+shift+w
    wait_for_confirmation_dialog 'Close this pane?'
    send_key esc
    prompts_before=$(prompt_count)
    send_key ctrl+c
    wait_for_prompt_count "$((prompts_before + 1))" 'the cancelled pane program prompt'
    record_active_pane 2
    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 2
    send_key ctrl+shift+w
    wait_for_confirmation_dialog 'Close this pane?'
    send_key enter
    record_active_pane 1

    send_key ctrl+shift+t
    wait_for_line_count "$GRAPHENE_E2E_PANE_IDS" 3 'the tab-close dialog fixture'
    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 3
    send_key ctrl+alt+w
    wait_for_confirmation_dialog 'Close this tab?'
    send_key esc
    prompts_before=$(prompt_count)
    send_key ctrl+c
    wait_for_prompt_count "$((prompts_before + 1))" 'the cancelled tab program prompt'
    record_active_pane 3
    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 3
    send_key ctrl+alt+w
    wait_for_confirmation_dialog 'Close this tab?'
    send_key enter
    record_active_pane 1

    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 1
    send_key ctrl+shift+q
    wait_for_confirmation_dialog 'Close the window?'
    send_key esc
    prompts_before=$(prompt_count)
    send_key ctrl+c
    wait_for_prompt_count "$((prompts_before + 1))" 'the cancelled window program prompt'
    record_active_pane 1
    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 1
    send_key ctrl+shift+q
    wait_for_confirmation_dialog 'Close the window?'
    send_key enter
    for _ in $(seq 1 200); do
      app_state=$(ps -o stat= -p "$app_pid" 2>/dev/null || true)
      [[ -z "$app_state" || "$app_state" == Z* ]] && break
      sleep 0.05
    done
    app_state=$(ps -o stat= -p "$app_pid" 2>/dev/null || true)
    [[ -n "$app_state" && "$app_state" != Z* ]] && {
      printf 'Window close confirmation did not close Graphene\n' >&2
      exit 1
    }
    printf '%s close dialog passed: cancel preserved shell; confirm closed the window\n' "$backend"
    ;;
  settings)
    config_before=$(sha256sum "$XDG_CONFIG_HOME/graphene/config.toml")
    send_key ctrl+comma
    if [[ "$backend" == x11 ]]; then
      settings_window=$(timeout 15 xdotool search --sync --onlyvisible --name 'Settings' | head -n1)
      xdotool windowfocus --sync "$settings_window"
    else
      settings_focused=false
      for _ in $(seq 1 200); do
        swaymsg -t get_tree | python3 -c '
import json, sys
def windows(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from windows(child)
tree = json.load(sys.stdin)
raise SystemExit(0 if any(w.get("name") == "Settings" and w.get("focused") for w in windows(tree)) else 1)
' && { settings_focused=true; break; }
        sleep 0.05
      done
      [[ "$settings_focused" == true ]] || {
        printf 'Settings did not receive focus after opening\n' >&2
        exit 1
      }
    fi
    # Sidebar positions are read from a capture of the Settings window, so pointer clicks
    # are offset by the window origin: X11 captures the window alone, while Sway places
    # the floating Settings window at the output origin and grim captures the output.
    settings_x=0
    settings_y=0
    if [[ "$backend" == x11 ]]; then
      eval "$(xdotool getwindowgeometry --shell "$settings_window" | grep -E '^(X|Y)=')"
      settings_x=$X
      settings_y=$Y
    fi
    capture_settings() {
      if [[ "$backend" == x11 ]]; then
        import -window "$settings_window" "$tmp/settings.png"
      else
        grim "$tmp/settings.png"
      fi
    }
    click_settings_section() {
      local section=$1
      local coordinates=
      for _ in $(seq 1 200); do
        capture_settings
        coordinates=$(ocr_text "$tmp/settings.png" tsv | python3 -c '
import sys

section = sys.argv[1]
for row in sys.stdin.read().splitlines()[1:]:
    columns = row.split("\t")
    if len(columns) == 12 and columns[11].strip().casefold() == section.casefold():
        left, top, width, height = (int(value) // 2 for value in columns[6:10])
        if left < 180 and float(columns[10]) > 20:
            print(left + width // 2, top + height // 2)
            break
else:
    raise SystemExit(1)
' "$section") && break
        sleep 0.05
      done
      [[ -n "$coordinates" ]] || {
        printf 'Could not locate Settings section %s\n' "$section" >&2
        return 1
      }
      read -r section_x section_y <<<"$coordinates"
      click_pointer "$((settings_x + section_x))" "$((settings_y + section_y))" left
      for _ in $(seq 1 200); do
        if [[ "$backend" == x11 ]]; then
          import -window "$settings_window" -crop 800x40+180+0 +repage "$tmp/settings-heading.png"
        else
          grim -g "$((settings_x + 180)),$settings_y 800x40" "$tmp/settings-heading.png"
        fi
        if ocr_text "$tmp/settings-heading.png" | head -n1 | grep -qi "$section"; then
          return 0
        fi
        sleep 0.05
      done
      printf 'Settings section heading %s was not visible after selection\n' "$section" >&2
      return 1
    }
    for section in Language Font Window Scrollback Cursor Selection Tabs Theme Keyboard Updates; do
      click_settings_section "$section"
    done
    # Reopening while Settings is open must reuse the existing window. Raising it is
    # not asserted: winit 0.30's Window::focus_window is a no-op on Wayland, and Xvfb has
    # no window manager to honour _NET_ACTIVE_WINDOW. The terminal command typed after
    # the shortcut is an ordering barrier proving the shortcut was handled.
    focus_main_window
    send_key ctrl+comma
    record_active_pane 1
    [[ $(settings_window_count) == 1 ]] || {
      printf 'Reopening Settings created a duplicate window\n' >&2
      exit 1
    }
    focus_settings_window
    send_key esc
    wait_for_settings_closed
    focus_main_window
    settings_config=$(sha256sum "$XDG_CONFIG_HOME/graphene/config.toml")
    [[ "$settings_config" == "$config_before" ]] || {
      printf 'Settings navigation unexpectedly changed config.toml\n' >&2
      exit 1
    }
    record_active_pane 1
    click_pointer 120 120 right
    for _ in $(seq 1 5); do
      send_key down
    done
    send_key enter
    wait_for_settings_open
    focus_settings_window
    send_key esc
    wait_for_settings_closed
    focus_main_window
    run_palette_action settings open_settings
    wait_for_settings_open
    focus_settings_window
    send_key esc
    wait_for_settings_closed
    focus_main_window
    record_active_pane 1
    printf '%s settings navigation passed: section heading observed, config unchanged, Escape returned to terminal\n' "$backend"
    ;;
  selection)
    show_fixture_text
    box=$(locate_word ALPHA)
    read -r alpha_x alpha_y alpha_width alpha_height <<<"$box"
    box=$(locate_word BRAVO)
    read -r bravo_x bravo_y bravo_width bravo_height <<<"$box"
    box=$(locate_word CHARLIE)
    read -r charlie_x charlie_y charlie_width charlie_height <<<"$box"
    box=$(locate_word ECHO)
    read -r echo_x echo_y echo_width echo_height <<<"$box"
    row1_y=$((bravo_y + bravo_height / 2))
    row2_y=$((echo_y + echo_height / 2))
    # The selected text is read back from the primary selection, so the words located by
    # OCR are also proven to be the text Graphene displays in those cells.
    expect_selection BRAVO 'a double click' \
      pointer_clicks "$(char_x "$bravo_x" "$bravo_width" 5 2)" "$row1_y" 2
    expect_selection 'DELTA ECHO FOXTROT' 'a triple click' \
      pointer_clicks "$(char_x "$echo_x" "$echo_width" 4 1)" "$row2_y" 3
    expect_selection 'ALPHA BRAVO' 'a drag' \
      pointer_drag "$(char_start_x "$alpha_x" "$alpha_width" 5 0)" "$row1_y" \
      "$(char_end_x "$bravo_x" "$bravo_width" 5 4)" "$row1_y"
    expect_selection 'ALPHA BRAVO CHARLIE' 'a Shift click' \
      pointer_clicks "$(char_end_x "$charlie_x" "$charlie_width" 7 6)" "$row1_y" 1 left shift
    expect_copy ctrl+shift+c 'ALPHA BRAVO CHARLIE' 'the copy shortcut'
    expect_selection "$(printf 'BRAVO\nECHO')" 'a Ctrl drag' \
      pointer_drag "$(char_start_x "$bravo_x" "$bravo_width" 5 0)" "$row1_y" \
      "$(char_end_x "$bravo_x" "$bravo_width" 5 4)" "$row2_y" ctrl
    expect_copy ctrl+shift+c "$(printf 'BRAVO\nECHO')" 'the copy shortcut on a block selection'
    # A click without drag clears the selection: the copy shortcut then leaves the
    # clipboard alone. The typed command afterwards proves the shortcut was handled.
    pointer_clicks "$(char_x "$charlie_x" "$charlie_width" 7 6)" "$((row2_y + 3 * (row2_y - row1_y)))" 1
    set_clipboard clipboard E2E_CLEARED
    send_key ctrl+shift+c
    record_active_pane 1
    [[ $(read_clipboard clipboard) == E2E_CLEARED ]] || {
      printf 'Copy after clearing the selection changed the clipboard to %q\n' \
        "$(read_clipboard clipboard)" >&2
      exit 1
    }
    printf '%s selection passed: word, line, drag, extension, block, clearing and copy\n' "$backend"
    ;;
  clipboard)
    # shellcheck disable=SC2016
    set_clipboard clipboard 'printf "%s\n" E2E_PASTE_KEYS >"$GRAPHENE_E2E_COMMAND_RESULT_FILE"'
    send_key ctrl+shift+v
    send_key enter
    wait_for_command_result E2E_PASTE_KEYS 'the command pasted with Ctrl+Shift+V'
    # shellcheck disable=SC2016
    set_clipboard clipboard 'printf "%s\n" E2E_PASTE_INSERT >"$GRAPHENE_E2E_COMMAND_RESULT_FILE"'
    send_key shift+insert
    send_key enter
    wait_for_command_result E2E_PASTE_INSERT 'the command pasted with Shift+Insert'
    show_fixture_text
    box=$(locate_word BRAVO)
    read -r bravo_x bravo_y bravo_width bravo_height <<<"$box"
    type_text 'printf "%s\n" '
    expect_selection BRAVO 'a double click' \
      pointer_clicks "$(char_x "$bravo_x" "$bravo_width" 5 2)" "$((bravo_y + bravo_height / 2))" 2
    read -r x y width height <<<"$(main_window_rect)"
    pointer_clicks "$((x + width / 2))" "$((y + height / 2))" 1 middle
    # shellcheck disable=SC2016
    type_text ' >"$GRAPHENE_E2E_COMMAND_RESULT_FILE"'
    send_key enter
    wait_for_command_result BRAVO 'the command completed by a middle-click paste'
    printf '%s clipboard passed: Ctrl+Shift+V, Shift+Insert and middle-click primary paste\n' "$backend"
    ;;
  scroll)
    run_shell_command 'echo E2E_TOP'
    run_shell_command 'cat "$GRAPHENE_E2E_ROWS"'
    # The first and last numbered rows the window shows, read with OCR.
    visible_rows() {
      window_text | grep -oE 'ROW [0-9]{3}' | awk '
        { n = $2 + 0; if (min == "" || n < min) min = n; if (n > max) max = n }
        END { if (min == "") exit 1; print min, max }'
    }
    wait_for_rows() {
      local condition=$1 description=$2 min= max=
      for _ in $(seq 1 60); do
        if read -r min max < <(visible_rows) && [[ -n "$min" ]] && eval "$condition"; then
          printf '%s %s\n' "$min" "$max"
          return 0
        fi
        sleep 0.05
      done
      printf 'The visible rows (%s to %s) did not satisfy %s after %s\n' \
        "${min:-none}" "${max:-none}" "$condition" "$description" >&2
      return 1
    }
    # The 300 rows exceed the view: the block is capped to the view and scrolls its own
    # output, and it pushes the first block out of the view.
    rows=$(wait_for_rows '((max == 300))' 'the rows command')
    read -r bottom_min bottom_max <<<"$rows"
    wait_for_window_text_gone 'E2E_TOP' 'The block above the capped block'
    read -r x y width height <<<"$(main_window_rect)"
    pointer_scroll "$((x + width / 2))" "$((y + height / 2))" 5
    wait_for_rows "((max < 300 && min < $bottom_min))" 'wheel up over the block' >/dev/null
    pointer_scroll "$((x + width / 2))" "$((y + height / 2))" -50
    wait_for_rows '((max == 300))' 'wheel down over the block' >/dev/null
    send_key ctrl+shift+pageup
    send_key shift+pageup
    wait_for_rows "((max < 300 && min < $bottom_min))" 'Shift+PageUp on the selected block' >/dev/null
    send_key shift+pagedown
    wait_for_rows '((max == 300))' 'Shift+PageDown on the selected block' >/dev/null
    # Shift+Home and Shift+End select in the prompt input; they scroll the history while
    # a command runs.
    type_text 'sleep 60'
    send_key enter
    wait_for_foreground_sleep 1
    send_key shift+home
    wait_for_window_text 'E2E_TOP' 'the first block after Shift+Home'
    send_key shift+end
    wait_for_window_text_gone 'E2E_TOP' 'The first block after Shift+End'
    wait_for_rows '((max == 300))' 'Shift+End' >/dev/null
    prompts_before=$(prompt_count)
    send_key ctrl+c
    wait_for_prompt_count "$((prompts_before + 1))" 'the interrupted sleep'
    printf '%s scrolling passed: wheel and Shift+PageUp/PageDown on a capped block, Shift+Home/End on the history\n' "$backend"
    ;;
  font)
    size=$(measure_active_pane 1)
    read -r base_rows base_columns <<<"$size"
    send_key ctrl+equal
    size=$(wait_for_columns_less 1 "$base_columns")
    read -r larger_rows larger_columns <<<"$size"
    ((larger_rows < base_rows)) || {
      printf 'A larger font kept %s rows\n' "$larger_rows" >&2
      exit 1
    }
    send_key ctrl+minus
    send_key ctrl+minus
    wait_for_columns_greater 1 "$base_columns" >/dev/null
    send_key ctrl+0
    wait_for_columns_equal_to 1 "$base_columns" >/dev/null
    printf '%s font size passed: increase, decrease and reset change the terminal grid\n' "$backend"
    ;;
  mouse)
    show_fixture_text
    box=$(locate_word ALPHA)
    read -r alpha_x alpha_y alpha_width alpha_height <<<"$box"
    box=$(locate_word BRAVO)
    read -r bravo_x bravo_y bravo_width bravo_height <<<"$box"
    row1_y=$((bravo_y + bravo_height / 2))
    # The program turns on SGR mouse reporting, then records the first press it reads.
    # shellcheck disable=SC2016
    type_text 'printf "\033[?1000h\033[?1006h"; : >"$GRAPHENE_E2E_MOUSE_READY"; IFS= read -rs -d M -t 30 press; IFS= read -rs -d m -t 5 release; printf "\033[?1000l\033[?1006l"; printf "%s\n" "${press#*<}" >"$GRAPHENE_E2E_MOUSE_REPORT"'
    send_key enter
    wait_for_file "$GRAPHENE_E2E_MOUSE_READY" 'the mouse-reporting program'
    # Shift keeps the selection for Graphene while the program captures the mouse.
    expect_selection BRAVO 'a Shift drag while the program captures the mouse' \
      pointer_drag "$(char_start_x "$bravo_x" "$bravo_width" 5 0)" "$row1_y" \
      "$(char_end_x "$bravo_x" "$bravo_width" 5 4)" "$row1_y" shift
    sentinel="E2E_SENTINEL_$RANDOM"
    set_clipboard primary "$sentinel"
    pointer_drag "$(char_start_x "$alpha_x" "$alpha_width" 5 0)" "$row1_y" \
      "$(char_end_x "$bravo_x" "$bravo_width" 5 4)" "$row1_y"
    wait_for_file "$GRAPHENE_E2E_MOUSE_REPORT" 'the reported mouse press'
    report=$(<"$GRAPHENE_E2E_MOUSE_REPORT")
    [[ "$report" == '0;1;1' ]] || {
      printf 'Expected a left press on column 1, row 1 to be reported; got %q\n' "$report" >&2
      exit 1
    }
    [[ $(read_clipboard primary) == "$sentinel" ]] || {
      printf 'A drag captured by the program also changed the selection\n' >&2
      exit 1
    }
    record_active_pane 1
    printf '%s mouse capture passed: reported press cell, no selection, Shift overrides\n' "$backend"
    ;;
  blocks)
    run_shell_command 'echo E2E_FIRST'
    run_shell_command "printf 'E2E_OUT_%s\\n' 1 2 3"
    send_key ctrl+shift+f
    wait_for_window_text '3 lines hidden' 'the folded last block'
    send_key ctrl+shift+f
    wait_for_window_text_gone 'hidden' 'The unfolded block marker'
    send_key ctrl+shift+alt+f
    wait_for_window_text '1 line hidden' 'the first block folded by fold all'
    wait_for_window_text '3 lines hidden' 'the last block folded by fold all'
    send_key ctrl+shift+alt+f
    wait_for_window_text_gone 'hidden' 'The marker of blocks unfolded by unfold all'
    send_key ctrl+shift+f
    box=$(locate_word hidden)
    read -r hidden_x hidden_y hidden_width hidden_height <<<"$box"
    pointer_clicks "$((hidden_x + hidden_width / 2))" "$((hidden_y + hidden_height / 2))" 1
    wait_for_window_text_gone 'hidden' 'The marker of a block unfolded by a click'
    expect_copy ctrl+shift+alt+o "$(printf 'E2E_OUT_1\nE2E_OUT_2\nE2E_OUT_3')" 'copying the unfolded output'
    printf '%s command blocks passed: fold, unfold, fold all, unfold all and click to unfold\n' "$backend"
    ;;
  shell-bash|shell-zsh|shell-fish)
    # Graphene's prompt editor sends Home as a sequence the integration binds.
    # shellcheck disable=SC2016
    type_text 'cho E2E_EDITED >"$GRAPHENE_E2E_COMMAND_RESULT_FILE"'
    send_key home
    type_text e
    send_key enter
    wait_for_command_result E2E_EDITED "the command edited at the $integration_shell prompt"
    run_shell_command 'echo E2E_FIRST'
    run_shell_command "printf 'E2E_OUT_%s\\n' 1 2 3"
    expect_copy ctrl+shift+alt+c "printf 'E2E_OUT_%s\\n' 1 2 3" 'copying the last command'
    expect_copy ctrl+shift+alt+o "$(printf 'E2E_OUT_1\nE2E_OUT_2\nE2E_OUT_3')" 'copying the last output'
    send_key ctrl+shift+pageup
    send_key ctrl+shift+pageup
    expect_copy ctrl+shift+alt+c 'echo E2E_FIRST' 'copying the command of the previous block'
    expect_copy ctrl+shift+alt+o E2E_FIRST 'copying the output of the previous block'
    send_key ctrl+shift+pagedown
    expect_copy ctrl+shift+alt+c "printf 'E2E_OUT_%s\\n' 1 2 3" 'copying the command of the next block'
    printf '%s %s integration passed: prompt editing, command blocks, navigation and copies\n' \
      "$backend" "$integration_shell"
    ;;
esac

stop_app
stop_pid "$sway_pid"
