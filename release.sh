#!/bin/sh
# Cuts a versioned Knack release: tests, builds Knack.app, packages .dmg & .zip,
# writes site/releases/latest.json, and optionally publishes to GitHub + Vercel.
# Usage: ./release.sh [--version X.Y.Z] [--notes "..."] [--skip-deploy]
set -eu

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

NEW_VERSION=""
NOTES=""
SKIP_DEPLOY=false

while [ $# -gt 0 ]; do
  case "$1" in
    --version)
      NEW_VERSION="${2:?missing version after --version}"
      shift 2
      ;;
    --notes)
      NOTES="${2:?missing text after --notes}"
      shift 2
      ;;
    --skip-deploy)
      SKIP_DEPLOY=true
      shift
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

if [ -n "$NEW_VERSION" ]; then
  sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml
  cargo update -p knack
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
TAG="v$VERSION"
if [ -z "$NOTES" ]; then
  NOTES="Knack $VERSION for macOS."
fi

cargo test
./bundle.sh --no-install

mkdir -p dist site/releases
ZIP_NAME="Knack-${VERSION}-arm64.zip"
DMG_NAME="Knack-${VERSION}-arm64.dmg"
ZIP_PATH="dist/$ZIP_NAME"
DMG_PATH="dist/$DMG_NAME"

rm -f "$ZIP_PATH" "$DMG_PATH"
ditto -c -k --sequesterRsrc --keepParent target/release/Knack.app "$ZIP_PATH"

STAGE="$(mktemp -d /tmp/knack-dmg.XXXXXX)"
trap 'rm -rf "$STAGE"' EXIT
ditto target/release/Knack.app "$STAGE/Knack.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Knack" -srcfolder "$STAGE" -ov -format UDZO "$DMG_PATH" >/dev/null

ZIP_SHA256="$(shasum -a 256 "$ZIP_PATH" | awk '{print $1}')"
DMG_SHA256="$(shasum -a 256 "$DMG_PATH" | awk '{print $1}')"
PUBLISHED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RELEASE_URL="https://github.com/jondcallahan/knack/releases/tag/$TAG"
DMG_URL="https://github.com/jondcallahan/knack/releases/download/$TAG/$DMG_NAME"
ZIP_URL="https://github.com/jondcallahan/knack/releases/download/$TAG/$ZIP_NAME"

python3 - "$VERSION" "$TAG" "$PUBLISHED_AT" "$NOTES" "$RELEASE_URL" "$DMG_URL" "$ZIP_URL" "$ZIP_SHA256" "$DMG_SHA256" <<'PY'
import json, sys
version, tag, published_at, notes, release_url, dmg_url, zip_url, zip_sha256, dmg_sha256 = sys.argv[1:]
manifest = {
    "version": version,
    "tag": tag,
    "published_at": published_at,
    "notes": notes,
    "release_url": release_url,
    "dmg_url": dmg_url,
    "zip_url": zip_url,
    "zip_sha256": zip_sha256,
    "dmg_sha256": dmg_sha256,
}
with open("site/releases/latest.json", "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PY

echo "Packaged $DMG_PATH and $ZIP_PATH; wrote site/releases/latest.json"

if ! $SKIP_DEPLOY; then
  if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "$DMG_PATH" "$ZIP_PATH" --clobber
  else
    gh release create "$TAG" "$DMG_PATH" "$ZIP_PATH" --title "Knack $VERSION" --notes "$NOTES"
  fi
  if [ -f site/index.html ]; then
    (cd site && vercel deploy --prod --yes)
  fi
fi
