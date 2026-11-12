#!/bin/bash
# Stagehand on Linux: build from this checkout and install to /usr/local.
#
#   git clone https://github.com/Cheter0410/stagehand.git && cd stagehand && ./install.sh
#
# Run it again later to update. Remove with: ./install.sh --uninstall
set -euo pipefail
cd "$(dirname "$0")"
say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

if [ "${1:-}" = "--uninstall" ]; then
  sudo rm -f /usr/local/bin/stagehand /usr/local/share/applications/stagehand.desktop \
    /usr/local/share/icons/hicolor/512x512/apps/stagehand.png
  echo "Stagehand removed (apps it installed stay)."
  exit 0
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

say "Installing what the build needs"
if command -v pacman >/dev/null 2>&1; then
  pkgs=(git base-devel gtk3 libxkbcommon-x11 libglvnd)
  command -v cargo >/dev/null 2>&1 || pkgs+=(rust)
  sudo pacman -S --needed --noconfirm "${pkgs[@]}"
elif command -v apt-get >/dev/null 2>&1; then
  sudo apt-get update && sudo apt-get install -y git build-essential pkg-config libgtk-3-dev libxkbcommon-x11-dev curl
elif command -v dnf >/dev/null 2>&1; then
  sudo dnf install -y git gcc pkgconf-pkg-config gtk3-devel libxkbcommon-x11-devel curl
fi
if ! command -v cargo >/dev/null 2>&1; then
  say "Installing Rust (rustup, for this user)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

say "Building Stagehand"
cargo build --release --locked
say "Installing to /usr/local"
sudo install -Dm755 target/release/stagehand /usr/local/bin/stagehand
sudo install -Dm644 packaging/linux/stagehand.desktop /usr/local/share/applications/stagehand.desktop
sudo install -Dm644 assets/icon.png /usr/local/share/icons/hicolor/512x512/apps/stagehand.png
say "Done"
echo "Start Stagehand from the application menu or with: stagehand"
