#!/bin/bash
# Stagehand on Linux: update this checkout, build and install - in one go, no sudo
# needed for the app itself.
#
#   git clone https://github.com/Cheter0410/stagehand.git && cd stagehand && ./install.sh
#
# Run it again later to update; only what is needed is done. The app goes into
# the folder all show tools share (~/.local/share/stagehand/apps/stagehand, as
# Stagehand installs it), with a menu entry and the command "stagehand".
# Remove with: ./install.sh --uninstall
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=scripts/ui.sh
. scripts/ui.sh
# shellcheck source=scripts/setup.sh
. scripts/setup.sh
setup_no_root

if [ "${1:-}" = "--uninstall" ]; then
  ui_header "Stagehand" "Entfernen"
  user_app_remove stagehand stagehand && ui_ok "Aus dem Stagehand-Ordner entfernt"
  if system_copy_there stagehand; then
    ui_sudo
    ui_run "Alte Kopie in /usr/local entfernen" system_copy_remove stagehand
  fi
  ui_ok "Entfernt (Einstellungen und eigene Dateien bleiben)"
  exit 0
fi

# --- Neuester Stand (danach läuft die neue Fassung dieses Skripts) ---------
if [ -z "${SM_PULLED:-}" ]; then
  ui_header "Stagehand" "Installieren / Aktualisieren"
  if setup_pull; then SM_PULLED=1 exec bash "$(pwd)/$(basename "$0")" "$@"; fi
fi

# --- Werkzeuge: nur was fehlt -------------------------------------------------
missing=()
if command -v pacman >/dev/null 2>&1; then
  pkgs=(git base-devel gtk3 libxkbcommon-x11 libglvnd)
  command -v cargo >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/cargo" ] || pkgs+=(rust)
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
  ui_warn "Unbekannte Distribution: Compiler, pkg-config und die GTK3-Entwicklungspakete selbst installieren"
fi
if [ ${#missing[@]} -gt 0 ]; then
  ui_sudo
  ui_run "Pakete installieren (${missing[*]})" install_pkgs "${missing[@]}"
else
  ui_ok "Benötigte Pakete sind da"
fi
setup_rust

V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
ui_section "Stagehand $V"
ui_run "Programm bauen" cargo build --release --locked
BIN=target/release/stagehand

# --- Installieren (nur wenn sich etwas geändert hat) -------------------------
if user_app_current stagehand stagehand "$BIN"; then
  ui_skip "Installieren" "Stagehand $V ist schon installiert"
elif [ -n "" ] && pgrep -x stagehand >/dev/null 2>&1; then
  ui_warn "Stagehand läuft gerade – nicht ersetzt (es könnte eine Show laufen)."
  ui_info "Beenden, dann nochmal: ./install.sh"
  ui_done
  exit 0
else
  ui_run "Installieren (Stagehand-Ordner)" user_app_install stagehand stagehand "$V" "$BIN" packaging/linux/stagehand.desktop assets/icon.png
fi
# A copy an older version of this script put into /usr/local would be found first.
if system_copy_there stagehand; then
  ui_sudo
  ui_run "Alte Kopie in /usr/local entfernen" system_copy_remove stagehand
fi
user_bin_on_path

ui_section "Fertig"
ui_ok "Starten: im Anwendungsmenü oder mit  stagehand"
ui_done
