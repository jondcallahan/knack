#!/bin/sh
# Builds "Knack.app" with its icon and optionally installs it into /Applications.
# Usage: ./bundle.sh [--open] [--no-install] [install-dir]
set -eu

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

OPEN_AFTER=false
INSTALL=true
INSTALL_DIR="/Applications"
for arg in "$@"; do
  case "$arg" in
    --open) OPEN_AFTER=true ;;
    --no-install) INSTALL=false ;;
    *) INSTALL_DIR="$arg" ;;
  esac
done

APP_NAME="Knack.app"
APP="target/release/$APP_NAME"
BUNDLE_ID="com.joncallahan.knack"
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

cargo build --release

ICONSET="target/release/AppIcon.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for px in 16 32 128 256 512; do
  sips -z $px $px assets/icon.png --out "$ICONSET/icon_${px}x${px}.png" >/dev/null
  sips -z $((px * 2)) $((px * 2)) assets/icon.png --out "$ICONSET/icon_${px}x${px}@2x.png" >/dev/null
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/knack "$APP/Contents/MacOS/knack"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Knack</string>
  <key>CFBundleDisplayName</key><string>Knack</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>knack</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign "$SIGN_IDENTITY" "$APP" 2>/dev/null || true

if ! $INSTALL; then
  echo "Built $APP ($VERSION)"
  exit 0
fi

DEST="$INSTALL_DIR/$APP_NAME"
RUNNING=false
if pgrep -qf "$DEST/Contents/MacOS/knack"; then
  RUNNING=true
  osascript -e "tell application id \"$BUNDLE_ID\" to quit" 2>/dev/null || true
  sleep 0.3
fi

mkdir -p "$INSTALL_DIR"
rm -rf "$DEST"
ditto "$APP" "$DEST"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST"
echo "Installed $DEST"

if $OPEN_AFTER || $RUNNING; then
  open "$DEST"
fi
