#!/usr/bin/env bash
# Capture the real Wayland window frame in a private nested Weston/Xvfb desktop.
# Ubuntu 22.04+: weston xvfb scrot libegl1-mesa libgl1-mesa-dri fonts-cantarell.
# Usage: bash packaging/linux/check-window-frame.sh target/release/vectorcraft target/window-frame
set -euo pipefail
umask 077

if [[ "$#" -ne 2 ]]; then
  echo 'Usage: check-window-frame.sh <VectorCraft executable> <artifact directory>' >&2
  exit 2
fi
for tool in Xvfb weston scrot realpath timeout; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
APP="$(realpath -- "$1")"
[[ -f "$APP" && -x "$APP" ]] || { echo 'VectorCraft must be a built executable.' >&2; exit 1; }
mkdir -p -- "$2"
OUTPUT="$(realpath -- "$2")"
TEMP_PARENT="$(realpath -- "${TMPDIR:-/tmp}")"
WORK="$(mktemp -d "$TEMP_PARENT/vectorcraft-window-frame.XXXXXXXX")"
XVFB_PID=''
WESTON_PID=''
APP_PID=''
PRIVATE_DISPLAY=''

alive() { [[ -n "$1" ]] && kill -0 "$1" 2>/dev/null; }
cleanup() {
  status=$?
  if [[ "$status" -ne 0 && -n "$PRIVATE_DISPLAY" ]] && alive "$XVFB_PID"; then
    timeout 5s env DISPLAY="$PRIVATE_DISPLAY" scrot -z -o "$OUTPUT/failure.png" 2>>"$OUTPUT/capture.log" || true
  fi
  for pid in "$APP_PID" "$WESTON_PID" "$XVFB_PID"; do
    if alive "$pid"; then kill "$pid" 2>/dev/null || true; fi
  done
  for _ in {1..30}; do
    if ! alive "$APP_PID" && ! alive "$WESTON_PID" && ! alive "$XVFB_PID"; then break; fi
    sleep 0.1
  done
  for pid in "$APP_PID" "$WESTON_PID" "$XVFB_PID"; do
    if alive "$pid"; then kill -KILL "$pid" 2>/dev/null || true; fi
    if [[ -n "$pid" ]]; then wait "$pid" 2>/dev/null || true; fi
  done
  case "$WORK" in "$TEMP_PARENT"/vectorcraft-window-frame.*) rm -rf -- "$WORK" ;; esac
  return "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$WORK/home" "$WORK/config" "$WORK/data" "$WORK/cache" "$WORK/runtime"
chmod 700 "$WORK/runtime"
# These overrides affect only the new child processes, never the user's session.
ISOLATED=(env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u DBUS_SESSION_BUS_ADDRESS
  "HOME=$WORK/home" "XDG_CONFIG_HOME=$WORK/config" "XDG_DATA_HOME=$WORK/data"
  "XDG_CACHE_HOME=$WORK/cache" "XDG_RUNTIME_DIR=$WORK/runtime" "XDG_SESSION_TYPE=wayland"
  "DBUS_SESSION_BUS_ADDRESS=unix:path=$WORK/no-session-bus" "LIBGL_ALWAYS_SOFTWARE=1" "WGPU_BACKEND=gl" "VECTORCRAFT_NO_PREFS=1")

Xvfb -displayfd 3 -screen 0 1920x1200x24 -nolisten tcp -noreset \
  3>"$WORK/display-number" >"$OUTPUT/xvfb.log" 2>&1 &
XVFB_PID=$!
for _ in {1..100}; do
  alive "$XVFB_PID" || { echo 'The private Xvfb server stopped.' >&2; exit 1; }
  if [[ -s "$WORK/display-number" ]]; then break; fi
  sleep 0.1
done
DISPLAY_NUMBER="$(cat "$WORK/display-number")"
[[ "$DISPLAY_NUMBER" =~ ^[0-9]+$ ]] || { echo 'Xvfb did not allocate a private display.' >&2; exit 1; }
PRIVATE_DISPLAY=":$DISPLAY_NUMBER"

# Ubuntu 22.04's Weston 9 supports these backend/socket/size flags.
# Keep the EGL renderer so the Wayland OpenGL client can use Mesa software rendering.
"${ISOLATED[@]}" "DISPLAY=$PRIVATE_DISPLAY" weston \
  --backend=x11-backend.so --socket=vectorcraft-frame --width=1920 --height=1200 \
  --idle-time=0 --no-config --log="$OUTPUT/weston.log" >"$OUTPUT/weston-stderr.log" 2>&1 &
WESTON_PID=$!
for _ in {1..150}; do
  alive "$WESTON_PID" || { echo 'The nested Wayland compositor stopped. See weston.log.' >&2; exit 1; }
  if [[ -S "$WORK/runtime/vectorcraft-frame" ]]; then break; fi
  sleep 0.1
done
[[ -S "$WORK/runtime/vectorcraft-frame" ]] || { echo 'Weston did not create its private Wayland socket.' >&2; exit 1; }

# DISPLAY stays unset, so a Wayland failure cannot silently fall back to X11.
"${ISOLATED[@]}" WAYLAND_DISPLAY=vectorcraft-frame WAYLAND_DEBUG=client \
  "$APP" >"$OUTPUT/vectorcraft.log" 2>&1 &
APP_PID=$!
READY=false
for _ in {1..300}; do
  alive "$APP_PID" || { echo 'VectorCraft stopped before its Wayland window was ready. See vectorcraft.log.' >&2; exit 1; }
  alive "$WESTON_PID" || { echo 'Weston stopped while opening VectorCraft.' >&2; exit 1; }
  if grep -Eq 'xdg_toplevel.*\.set_title\(.*VectorCraft' "$OUTPUT/vectorcraft.log" \
    && grep -Eq 'xdg_surface.*\.ack_configure\(' "$OUTPUT/vectorcraft.log" \
    && grep -Eq 'wl_surface.*\.attach\(.*wl_buffer' "$OUTPUT/vectorcraft.log" \
    && grep -Eq 'wl_surface.*\.commit\(' "$OUTPUT/vectorcraft.log"; then
    READY=true
    break
  fi
  sleep 0.1
done
[[ "$READY" == true ]] || { echo 'VectorCraft did not render a configured Wayland window within 30 seconds.' >&2; exit 1; }
sleep 3
alive "$APP_PID" && alive "$WESTON_PID" && alive "$XVFB_PID" \
  || { echo 'The private desktop or VectorCraft stopped before capture.' >&2; exit 1; }
timeout 10s env DISPLAY="$PRIVATE_DISPLAY" scrot -z -o "$OUTPUT/window-frame.png" >"$OUTPUT/capture.log" 2>&1
[[ -s "$OUTPUT/window-frame.png" ]] || { echo 'The native frame screenshot is missing.' >&2; exit 1; }
printf 'Rendered VectorCraft over Wayland, including its native frame: %s/window-frame.png\n' "$OUTPUT"
