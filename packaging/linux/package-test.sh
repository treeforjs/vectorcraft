#!/usr/bin/env bash
# Exercise package metadata and compatibility aliases without building or installing the app.
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_PARENT="$(cd -- "${TMPDIR:-/tmp}" && pwd -P)"
WORK="$(mktemp -d "$TEMP_PARENT/vectorcraft-package-check.XXXXXXXX")"
cleanup() {
  case "$WORK" in "$TEMP_PARENT"/vectorcraft-package-check.*) rm -rf -- "$WORK" ;; esac
}
trap cleanup EXIT
mkdir -p "$WORK/target/release" "$WORK/tools" "$WORK/dist" "$WORK/extracted"
for name in vectorcraft vectorcraft-cli; do
  cat >"$WORK/target/release/$name" <<'BINARY'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == --version ]] || exit 1
printf 'VectorCraft package fixture\n'
BINARY
  chmod 755 "$WORK/target/release/$name"
done
cat >"$WORK/tools/appimagetool" <<'TOOL'
#!/usr/bin/env bash
set -euo pipefail
appdir="${@: -2:1}"
output="${@: -1}"
current=io.github.treeforjs.vectorcraft
legacy=ai.storyteller.vectorcraft
test -f "$appdir/$current.desktop"
test -f "$appdir/$current.png"
test ! -d "$appdir/usr/share/doc"
test -s "$appdir/usr/share/licenses/vectorcraft/OFL-Cantarell.txt"
grep -Fq 'Copyright (c) 2022' "$appdir/usr/share/licenses/vectorcraft/NOTICE"
grep -Fxq 'NoDisplay=true' "$appdir/usr/share/applications/$legacy.desktop"
cmp "$appdir/usr/share/icons/hicolor/256x256/apps/$current.png" \
  "$appdir/usr/share/icons/hicolor/256x256/apps/$legacy.png"
printf 'AppImage staging fixture\n' >"$output"
printf 'AppImage update staging fixture\n' >"$output.zsync"
TOOL
chmod 755 "$WORK/tools/appimagetool"
CARGO_TARGET_DIR="$WORK/target" DIST="$WORK/dist" APPIMAGETOOL="$WORK/tools/appimagetool" \
  bash "$ROOT/packaging/linux/package.sh" --skip-build --formats 'tar appimage'
archives=("$WORK/dist/"*.tar.gz)
[[ "${#archives[@]}" -eq 1 && -f "${archives[0]}" ]]
tar -xzf "${archives[0]}" --no-same-owner -C "$WORK/extracted"
packages=("$WORK/extracted/"*)
[[ "${#packages[@]}" -eq 1 ]]
share="${packages[0]}/share"
grep -Fxq 'Icon=io.github.treeforjs.vectorcraft' "$share/applications/io.github.treeforjs.vectorcraft.desktop"
if grep -Fxq 'NoDisplay=true' "$share/applications/io.github.treeforjs.vectorcraft.desktop"; then
  echo 'The current launcher must remain visible.' >&2
  exit 1
fi
grep -Fxq 'NoDisplay=true' "$share/applications/ai.storyteller.vectorcraft.desktop"
cmp "$ROOT/NOTICE" "$share/doc/vectorcraft/NOTICE"
cmp "$ROOT/assets/fonts/OFL-Cantarell.txt" "$share/doc/vectorcraft/OFL-Cantarell.txt"
printf 'Package metadata, hidden legacy aliases and retained licences passed.\n'
