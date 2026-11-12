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
[ "$(uname -s)" = "Darwin" ] || ui_fail "Dieses Skript ist für macOS (Linux: ./install.sh)."
[ -n "${SM_PULLED:-}" ] || ui_header "Stagehand" "Installieren / Aktualisieren"

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  ui_fail "Im Fenster, das sich geöffnet hat, auf 'Installieren' klicken und warten. Dann nochmal: bash install-mac.sh"
fi
if [ -z "${SM_PULLED:-}" ] && [ -d .git ]; then
  if git diff --quiet && git diff --cached --quiet; then
    git fetch -q 2>/dev/null || true
    if [ -n "$(git rev-list HEAD..@{u} 2>/dev/null)" ]; then
      ui_run "Neuesten Stand holen" git pull -q --ff-only
      SM_PULLED=1 exec bash "$(pwd)/$(basename "$0")" "$@"
    fi
    ui_ok "Quellcode ist aktuell"
  else
    ui_warn "Lokale Änderungen – baue den Stand, der hier liegt"
  fi
else
  ui_ok "Quellcode ist aktuell"
fi
if [ ! -x "$HOME/.cargo/bin/rustup" ] && ! command -v rustup >/dev/null 2>&1; then
  ui_run "Rust installieren (einmalig)" sh -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

ui_section "Bauen"
./packaging/macos/build-dmg.sh
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
DMG="dist/Stagehand-$V.dmg"

ui_section "Installieren"
[ -n "${NO_INSTALL:-}" ] && { ui_ok "Installer: $DMG"; ui_done; exit 0; }
STAMP=dist/.made/installed-app
WANT=$(cat "dist/.made/$(basename "$DMG")" 2>/dev/null || echo none)
if [ -d /Applications/Stagehand.app ] && [ "$(cat "$STAMP" 2>/dev/null)" = "$WANT" ]; then
  ui_skip "In /Applications" "Stagehand $V ist schon installiert"
else
  put() {
    pkill -x stagehand 2>/dev/null || true
    MNT=$(mktemp -d)
    hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$DMG" >/dev/null
    rm -rf /Applications/Stagehand.app
    ditto "$MNT/Stagehand.app" /Applications/Stagehand.app
    hdiutil detach "$MNT" -quiet >/dev/null 2>&1 || true
    rmdir "$MNT" 2>/dev/null || true
    xattr -dr com.apple.quarantine /Applications/Stagehand.app 2>/dev/null || true
  }
  ui_run "In /Applications" put
  printf '%s' "$WANT" > "$STAMP"
fi

ui_section "Fertig"
ui_ok "Stagehand ist in Programme (Launchpad, Spotlight)"
ui_done
open /Applications/Stagehand.app
