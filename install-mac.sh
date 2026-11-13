#!/bin/bash
# Stagehand on the Mac: update this checkout, build the DMG and install the app.
#
#   mkdir -p ~/Desktop/dev
#   git clone https://github.com/Cheter0410/stagehand.git ~/Desktop/dev/stagehand
#   cd ~/Desktop/dev/stagehand
#   bash install-mac.sh
#
# Run it again later to update; only what is needed is done. Normally you do
# not need this: Stagehand updates itself.
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=scripts/ui.sh
. scripts/ui.sh
# shellcheck source=scripts/setup.sh
. scripts/setup.sh
[ "$(uname -s)" = "Darwin" ] || ui_fail "Dieses Skript ist für macOS (Linux: ./install.sh)."
setup_no_root
[ -n "${SM_PULLED:-}" ] || ui_header "Stagehand" "Installieren / Aktualisieren"

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  ui_fail "Im Fenster, das sich geöffnet hat, auf 'Installieren' klicken und warten. Dann nochmal: bash install-mac.sh"
fi
if [ -z "${SM_PULLED:-}" ]; then
  if setup_pull; then SM_PULLED=1 exec bash "$(pwd)/$(basename "$0")" "$@"; fi
fi
setup_rust

ui_section "Bauen"
./packaging/macos/build-dmg.sh
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
DMG="dist/Stagehand-$V.dmg"

ui_section "Installieren"
[ -n "${NO_INSTALL:-}" ] && { ui_ok "Installer: $DMG"; ui_done; exit 0; }
STAMP=dist/.made/installed-app
# All show tools share one folder (as Stagehand installs them).
APPDIR=$(mac_app_dir)
WANT=$(cat "dist/.made/$(basename "$DMG")" 2>/dev/null || echo none)
if [ -d "$APPDIR/Stagehand.app" ] && [ "$(cat "$STAMP" 2>/dev/null)" = "$WANT" ]; then
  ui_skip "In $APPDIR" "Stagehand $V ist schon installiert"
else
  pkill -x stagehand 2>/dev/null || true
  ui_run "In $APPDIR" mac_app_put "$DMG" Stagehand.app "$APPDIR"
  printf '%s' "$WANT" > "$STAMP"
fi

ui_section "Fertig"
ui_ok "Stagehand ist in Programme › Stagehand (Launchpad, Spotlight)"
ui_done
open "$APPDIR/Stagehand.app"
