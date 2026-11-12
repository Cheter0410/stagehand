#!/bin/bash
# Stagehand on the Mac: update this checkout, build the DMG and install the app.
#
#   mkdir -p ~/Desktop/dev
#   git clone https://github.com/Cheter0410/stagehand.git ~/Desktop/dev/stagehand
#   cd ~/Desktop/dev/stagehand
#   bash install-mac.sh
#
# Run it again later to update. Missing tools (Xcode command line tools, Rust)
# are installed. Normally you do not need this: Stagehand updates itself.
set -euo pipefail
cd "$(dirname "$0")"
[ "$(uname -s)" = "Darwin" ] || { echo "This script is for macOS (Linux: ./install.sh)."; exit 1; }
say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  echo ">>> In the window that opened, click 'Install'; then run this again: bash install-mac.sh"
  exit 1
fi
if [ -z "${SM_PULLED:-}" ] && [ -d .git ]; then
  if git diff --quiet && git diff --cached --quiet; then
    say "Fetching the newest version"
    git pull --ff-only || echo "(could not update - building the version that is here)"
    SM_PULLED=1 exec bash "$(pwd)/$(basename "$0")" "$@"
  else
    echo "(local changes in $(pwd) - not updating)"
  fi
fi
if [ ! -x "$HOME/.cargo/bin/rustup" ] && ! command -v rustup >/dev/null 2>&1; then
  say "Installing Rust"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

say "Building Stagehand (takes a few minutes the first time)"
./packaging/macos/build-dmg.sh
DMG=$(ls -t dist/Stagehand-*.dmg | head -n 1)
[ -n "${NO_INSTALL:-}" ] && { open dist; say "Done: $DMG"; exit 0; }

say "Installing into /Applications"
pkill -x stagehand 2>/dev/null || true
MNT=$(mktemp -d)
hdiutil attach -nobrowse -readonly -mountpoint "$MNT" "$DMG" >/dev/null
trap 'hdiutil detach "$MNT" -quiet >/dev/null 2>&1 || true; rmdir "$MNT" 2>/dev/null || true' EXIT
rm -rf /Applications/Stagehand.app
ditto "$MNT/Stagehand.app" /Applications/Stagehand.app
xattr -dr com.apple.quarantine /Applications/Stagehand.app 2>/dev/null || true
say "Done"
echo "Stagehand is in Programme / Applications (Launchpad, Spotlight)."
open /Applications/Stagehand.app
