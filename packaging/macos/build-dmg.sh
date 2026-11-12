#!/bin/bash
# Builds dist/Stagehand-x.y.z.dmg on a Mac, for this Mac's architecture
# (one compile). UNIVERSAL=1 builds Apple Silicon + Intel.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ "$(uname -s)" = "Darwin" ] || { echo "This script must run on macOS."; exit 1; }
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
command -v cargo >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p dist && touch dist/.build-start

if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then HOST=arm64; else HOST=x86_64; fi
ARCHS="${ARCHS:-$HOST}"
[ -n "${UNIVERSAL:-}" ] && ARCHS="arm64 x86_64"
BINS=()
for a in $ARCHS; do
  case "$a" in
    arm64|aarch64) a=arm64; t=aarch64-apple-darwin ;;
    *) a=x86_64; t=x86_64-apple-darwin ;;
  esac
  if [ "$a" = "$HOST" ] && [ "$(uname -m)" = "$HOST" ]; then
    echo "== Building for this Mac ($a)"
    cargo build --release --locked
    BINS+=("target/release/stagehand")
  else
    echo "== Building for $t"
    rustup target add "$t"
    cargo build --release --locked --target "$t"
    BINS+=("target/$t/release/stagehand")
  fi
done

V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
WORK=$(mktemp -d)
APP="$WORK/dmg/Stagehand.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/stagehand" "${BINS[@]}"
sed "s/APP_VERSION/$V/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
ICONSET="$WORK/Stagehand.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Stagehand.icns"
# Ad-hoc signature: macOS asks once for "Open Anyway".
codesign --force --deep -s - "$APP"
ln -s /Applications "$WORK/dmg/Applications"
rm -f "dist/Stagehand-$V.dmg"
hdiutil create -volname "Stagehand" -srcfolder "$WORK/dmg" -ov -format UDZO "dist/Stagehand-$V.dmg"
rm -rf "$WORK"
echo "Built dist/Stagehand-$V.dmg"
