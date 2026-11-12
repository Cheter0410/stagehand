#!/bin/bash
# Builds Stagehand for Linux and Windows on one Linux PC:
#   dist/stagehand-<v>-linux-x86_64.tar.gz   portable Linux program
#   dist/Stagehand-Setup-<v>.exe             Windows installer (Inno Setup under Wine)
# One-time setup (Arch): see README.md. SKIP_WINDOWS=1 skips Windows.
# Releases: ./release.sh (builds with this script and uploads).
set -euo pipefail
cd "$(dirname "$0")/.."
V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p dist && touch dist/.build-start

echo "== Tests"
cargo test --locked

echo "== Linux program"
cargo build --release --locked
NAME="stagehand-$V-linux-x86_64"
rm -rf "dist/$NAME" && mkdir -p "dist/$NAME"
cp target/release/stagehand README.md packaging/linux/stagehand.desktop "dist/$NAME/"
cp assets/icon.png "dist/$NAME/stagehand.png"
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME" && rm -rf "dist/$NAME"

if [ -z "${SKIP_WINDOWS:-}" ]; then
  echo "== Windows program"
  rustup target add x86_64-pc-windows-gnu >/dev/null
  cargo build --release --locked --target x86_64-pc-windows-gnu
  EXE=target/x86_64-pc-windows-gnu/release/stagehand.exe
  echo "== Windows installer"
  ISCC="${ISCC:-$HOME/.wine/drive_c/InnoSetup/ISCC.exe}"
  WINE=$(command -v wine || command -v wine64 || true)
  export WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
  if [ -n "$WINE" ] && [ -f "$ISCC" ]; then
    rm -rf stage && mkdir -p stage && cp "$EXE" README.md stage/
    "$WINE" "$ISCC" "/DAppVersion=$V" "Z:$(pwd)/packaging/windows/stagehand.iss"
    rm -rf stage
  else
    echo "Wine or Inno Setup ($ISCC) not found - skipping the Windows installer."
  fi
fi

ls -la dist
