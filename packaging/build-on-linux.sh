#!/bin/bash
# Builds Stagehand for Linux and Windows on one Linux PC:
#   dist/stagehand-<v>-linux-x86_64.tar.gz   portable Linux program
#   dist/Stagehand-Setup-<v>.exe             Windows installer (Inno Setup under Wine)
# The list of what belongs to this build: dist/files-linux-pc.txt (release.sh
# uploads exactly these). Nothing is done twice: tests once per commit,
# packages only when the program changed. SKIP_WINDOWS=1 skips Windows.
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=../scripts/ui.sh
. scripts/ui.sh
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p dist target
FILES=()
ui_header "Stagehand $V" "Linux + Windows bauen"

key=""
[ -z "$(git status --porcelain --untracked-files=no 2>/dev/null)" ] && key=$(git rev-parse HEAD 2>/dev/null || true)
if [ -n "$key" ] && [ -f "target/.tested/$key" ]; then
  ui_skip "Tests" "für diesen Stand schon bestanden"
else
  ui_run "Tests" cargo test --locked
  [ -n "$key" ] && mkdir -p target/.tested && touch "target/.tested/$key"
fi

ui_run "Linux-Programm" cargo build --release --locked
BIN=target/release/stagehand
KEY="$V-$(ui_fingerprint "$BIN")"
NAME="stagehand-$V-linux-x86_64"
TAR="dist/$NAME.tar.gz"
make_tar() {
  rm -rf "dist/$NAME" && mkdir -p "dist/$NAME"
  cp "$BIN" README.md packaging/linux/stagehand.desktop "dist/$NAME/"
  cp assets/icon.png "dist/$NAME/stagehand.png"
  tar -C dist -czf "$TAR" "$NAME" && rm -rf "dist/$NAME"
}
if ui_fresh "$TAR" "$KEY"; then ui_skip "Linux-Archiv" "unverändert"; else
  ui_run "Linux-Archiv" make_tar && ui_made "$TAR" "$KEY"
fi
FILES+=("$TAR")

if [ -z "${SKIP_WINDOWS:-}" ]; then
  ui_run "Windows-Programm" cargo build --release --locked --target x86_64-pc-windows-gnu
  EXE=target/x86_64-pc-windows-gnu/release/stagehand.exe
  WKEY="$V-$(ui_fingerprint "$EXE")"
  ISCC="${ISCC:-$HOME/.wine/drive_c/InnoSetup/ISCC.exe}"
  WINE=$(command -v wine || command -v wine64 || true)
  export WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
  SETUP="dist/Stagehand-Setup-$V.exe"
  make_setup() {
    rm -rf stage && mkdir -p stage && cp "$EXE" README.md stage/
    "$WINE" "$ISCC" "/DAppVersion=$V" "Z:$(pwd)/packaging/windows/stagehand.iss"
    rm -rf stage
  }
  if [ -n "$WINE" ] && [ -f "$ISCC" ]; then
    if ui_fresh "$SETUP" "$WKEY"; then ui_skip "Windows-Installer" "unverändert"; else
      ui_run "Windows-Installer" make_setup && ui_made "$SETUP" "$WKEY"
    fi
    FILES+=("$SETUP")
  else
    ui_warn "Windows-Installer übersprungen: Wine oder Inno Setup fehlt (release.sh richtet beides ein)"
  fi
fi

{ echo "$V"; printf '%s\n' "${FILES[@]}"; } > dist/files-linux-pc.txt
ui_done
