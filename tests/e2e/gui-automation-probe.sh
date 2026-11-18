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
  smoke|tabs|palette|panes|menu|dialogs|settings) ;;
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
    for command in convert import tesseract xdotool; do
      command -v "$command" >/dev/null || {
        printf 'Required command not found: %s\n' "$command" >&2
        exit 2
      }
    done
    ;;
  wayland)
    for command in convert grim sway swaymsg python3 tesseract wtype; do
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
export SHELL="$tmp/shell"
export GRAPHENE_SHELL_INTEGRATION=0

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
esac

stop_app
stop_pid "$sway_pid"
