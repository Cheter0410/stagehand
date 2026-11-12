#!/bin/bash
# Stagehand on Linux: update this checkout, build and install to /usr/local.
#
#   git clone https://github.com/Cheter0410/stagehand.git && cd stagehand && ./install.sh
#
# Run it again later to update; only what is needed is done.
# Remove with: ./install.sh --uninstall
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=scripts/ui.sh
. scripts/ui.sh

if [ "${1:-}" = "--uninstall" ]; then
  ui_header "Stagehand" "Entfernen"
  ui_sudo
  ui_run "Dateien entfernen" sudo rm -f /usr/local/bin/stagehand /usr/local/share/applications/stagehand.desktop \
    /usr/local/share/icons/hicolor/512x512/apps/stagehand.png
  ui_ok "Entfernt (installierte Apps bleiben)"
  exit 0
fi

if [ -z "${SM_PULLED:-}" ] && [ -d .git ]; then
  ui_header "Stagehand" "Installieren / Aktualisieren"
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

missing=()
if command -v pacman >/dev/null 2>&1; then
  pkgs=(git base-devel gtk3 libxkbcommon-x11 libglvnd)
  command -v cargo >/dev/null 2>&1 || pkgs+=(rust)
  while IFS= read -r p; do [ -n "$p" ] && missing+=("$p"); done < <(pacman -T "${pkgs[@]}" 2>/dev/null || true)
  install_pkgs() { sudo pacman -S --needed --noconfirm "$@"; }
elif command -v apt-get >/dev/null 2>&1; then
  for p in git build-essential pkg-config libgtk-3-dev libxkbcommon-x11-dev curl; do
    dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "install ok installed" || missing+=("$p")
  done
  install_pkgs() { sudo apt-get update && sudo apt-get install -y "$@"; }
elif command -v dnf >/dev/null 2>&1; then
  for p in git gcc pkgconf-pkg-config gtk3-devel libxkbcommon-x11-devel curl; do
    rpm -q "$p" >/dev/null 2>&1 || missing+=("$p")
  done
  install_pkgs() { sudo dnf install -y "$@"; }
else
  install_pkgs() { return 1; }
fi
if [ ${#missing[@]} -gt 0 ]; then
  ui_sudo
  ui_run "Pakete installieren (${missing[*]})" install_pkgs "${missing[@]}"
else
  ui_ok "Benötigte Pakete sind da"
fi
if ! command -v cargo >/dev/null 2>&1; then
  ui_run "Rust installieren (einmalig)" sh -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal"
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ui_section "Stagehand $V"
ui_run "Programm bauen" cargo build --release --locked
if cmp -s /usr/local/bin/stagehand target/release/stagehand; then
  ui_skip "Installieren" "Stagehand $V ist schon installiert"
else
  put() {
    sudo install -Dm755 target/release/stagehand /usr/local/bin/stagehand
    sudo install -Dm644 packaging/linux/stagehand.desktop /usr/local/share/applications/stagehand.desktop
    sudo install -Dm644 assets/icon.png /usr/local/share/icons/hicolor/512x512/apps/stagehand.png
  }
  ui_sudo
  ui_run "Installieren nach /usr/local" put
fi

ui_section "Fertig"
ui_ok "Starten: im Anwendungsmenü oder mit  stagehand"
ui_done
