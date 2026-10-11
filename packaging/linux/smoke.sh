#!/usr/bin/env bash
# Exercise installed binaries in a desktop session, or with dbus-run-session + xvfb-run.
# Overrides: VECTORCRAFT_BIN, VECTORCRAFT_CLI, SMOKE_OUT, VECTORCRAFT_SMOKE_PORT.
set -euo pipefail
APP="${VECTORCRAFT_BIN:-vectorcraft}"
CLI="${VECTORCRAFT_CLI:-vectorcraft-cli}"
OUT="${SMOKE_OUT:-$(mktemp -d -t vectorcraft-smoke.XXXXXX)}"
PORT="${VECTORCRAFT_SMOKE_PORT:-17979}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
for file in input.svg output.png output.pdf original.json edited.vectorcraft reopened.json desktop.png desktop.log control.jsonl; do
  if [ -e "$OUT/$file" ] || [ -L "$OUT/$file" ]; then
    echo "error: smoke output already exists: $OUT/$file (use a fresh SMOKE_OUT directory)" >&2
    exit 1
  fi
done
command -v jq >/dev/null || { echo "error: jq is required" >&2; exit 1; }
"$APP" --version
"$CLI" --version
cat >"$OUT/input.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" width="128" height="96" viewBox="0 0 128 96">
  <rect x="8" y="8" width="112" height="80" fill="#df6432"/>
  <circle cx="64" cy="48" r="24" fill="#234a89"/>
</svg>
SVG
"$CLI" info "$OUT/input.svg" >"$OUT/original.json"
"$CLI" convert "$OUT/input.svg" "$OUT/output.png"
"$CLI" convert "$OUT/input.svg" "$OUT/output.pdf"
test -s "$OUT/output.png"
test -s "$OUT/output.pdf"

: >"$OUT/desktop.log"
VECTORCRAFT_NO_PREFS=1 "$APP" --control "$PORT" "$OUT/input.svg" >"$OUT/desktop.log" 2>&1 &
APP_PID=$!
cleanup() {
  kill "$APP_PID" 2>/dev/null || true
  wait "$APP_PID" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
CONNECTED=0
for ((attempt = 0; attempt < 60; attempt++)); do
  if ! kill -0 "$APP_PID" 2>/dev/null; then cat "$OUT/desktop.log" >&2; exit 1; fi
  # A different instance may already own the port: only drive the process we just started.
  if grep -Fq "vectorcraft: control server listening on 127.0.0.1:$PORT" "$OUT/desktop.log" \
    && { exec 3<>"/dev/tcp/127.0.0.1/$PORT"; } 2>/dev/null; then CONNECTED=1; break; fi
  sleep 0.5
done
if [ "$CONNECTED" != 1 ]; then cat "$OUT/desktop.log" >&2; echo "desktop did not start" >&2; exit 1; fi

rpc() {
  local reply
  printf '%s\n' "$1" >&3
  if ! IFS= read -r -t 30 reply <&3; then
    cat "$OUT/desktop.log" >&2
    echo "desktop control request timed out" >&2
    return 1
  fi
  printf '%s\n' "$reply" | tee -a "$OUT/control.jsonl" | jq -e '.ok == true' >/dev/null
}
rpc '{"id":1,"method":"ui.inspect","params":{}}'
jq -e 'select(.id == 1) | .result.documents | length == 1' "$OUT/control.jsonl" >/dev/null
rpc '{"id":2,"method":"engine.execute","params":{"command":"shape.rectangle","params":{"x":16,"y":16,"width":32,"height":24}}}'
rpc "$(jq -nc --arg path "$OUT/edited.vectorcraft" '{id:3,method:"engine.execute",params:{command:"document.save",params:{path:$path}}}')"
for ((attempt = 0; attempt < 60; attempt++)); do
  if [ -s "$OUT/edited.vectorcraft" ]; then break; fi
  sleep 0.5
done
test -s "$OUT/edited.vectorcraft"
"$CLI" info "$OUT/edited.vectorcraft" >"$OUT/reopened.json"
jq -e --slurpfile original "$OUT/original.json" \
  '.info.objects == ($original[0].info.objects + 1) and
   (.kinds.Rectangle // 0) == (($original[0].kinds.Rectangle // 0) + 1)' "$OUT/reopened.json" >/dev/null
rpc "$(jq -nc --arg path "$OUT/desktop.png" '{id:4,method:"ui.screenshot",params:{path:$path}}')"
test -s "$OUT/desktop.png"
echo "VectorCraft desktop, edit/save/reopen and CLI smoke passed: $OUT"
