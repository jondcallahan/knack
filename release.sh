#!/bin/sh
# Cuts a versioned Knack release from a clean, tagged commit on main: runs tests,
# builds Knack.app, signs & notarizes (unless --allow-unsigned-release), packages
# .dmg & .zip, writes site/releases/latest.json, and publishes to GitHub + Vercel.
# Usage: ./release.sh [--notes "..."] [--allow-unsigned-release] [--skip-deploy]
set -eu

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

NOTES=""
SKIP_DEPLOY=false
ALLOW_UNSIGNED=false
SIGN_IDENTITY="${SIGN_IDENTITY:--}"
NOTARY_PROFILE="${NOTARY_PROFILE:-}"

while [ $# -gt 0 ]; do
  case "$1" in
    --notes)
      NOTES="${2:?missing text after --notes}"
      shift 2
      ;;
    --skip-deploy)
      SKIP_DEPLOY=true
      shift
      ;;
    --allow-unsigned-release)
      ALLOW_UNSIGNED=true
      shift
      ;;
    *)
      echo "Unknown option: $1 (bump Cargo.toml, commit, and tag vX.Y.Z before running release.sh)" >&2
      exit 1
      ;;
  esac
done

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
TAG="v$VERSION"
if [ -z "$NOTES" ]; then
  NOTES="Knack $VERSION for macOS."
fi

if ! $ALLOW_UNSIGNED; then
  if [ "$SIGN_IDENTITY" = "-" ] || [ -z "$NOTARY_PROFILE" ]; then
    echo "Refusing unnotarized release: set SIGN_IDENTITY (Developer ID Application) and NOTARY_PROFILE, or pass --allow-unsigned-release." >&2
    exit 1
  fi
fi

if ! $SKIP_DEPLOY; then
  if [ -n "$(git status --porcelain --untracked-files=all -- ':!site/releases/latest.json')" ]; then
    echo "Working tree has uncommitted or untracked files. Clean or commit before publishing." >&2
    exit 1
  fi
  HEAD_SHA="$(git rev-parse HEAD)"
  TAG_SHA="$(git rev-parse --verify "refs/tags/$TAG^{commit}" 2>/dev/null || true)"
  if [ "$TAG_SHA" != "$HEAD_SHA" ]; then
    echo "Tag $TAG must exist and point to HEAD ($HEAD_SHA). Run: git tag $TAG && git push origin main $TAG" >&2
    exit 1
  fi
  REMOTE_SHA="$(git ls-remote origin "refs/tags/$TAG^{}" "refs/tags/$TAG" | tail -1 | awk '{print $1}')"
  if [ "$REMOTE_SHA" != "$HEAD_SHA" ]; then
    echo "Remote tag $TAG on origin does not match HEAD ($HEAD_SHA). Push commits and tag first." >&2
    exit 1
  fi
fi

cargo test
SIGN_IDENTITY="$SIGN_IDENTITY" ./bundle.sh --no-install

mkdir -p dist site/releases
ZIP_NAME="Knack-${VERSION}-arm64.zip"
DMG_NAME="Knack-${VERSION}-arm64.dmg"
ZIP_PATH="dist/$ZIP_NAME"
DMG_PATH="dist/$DMG_NAME"

rm -f "$ZIP_PATH" "$DMG_PATH"
ditto -c -k --sequesterRsrc --keepParent target/release/Knack.app "$ZIP_PATH"

if [ -n "$NOTARY_PROFILE" ]; then
  xcrun notarytool submit "$ZIP_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple target/release/Knack.app
  spctl --assess --type execute --verbose target/release/Knack.app
  rm -f "$ZIP_PATH"
  ditto -c -k --sequesterRsrc --keepParent target/release/Knack.app "$ZIP_PATH"
fi

STAGE="$(mktemp -d /tmp/knack-dmg.XXXXXX)"
trap 'rm -rf "$STAGE"' EXIT
ditto target/release/Knack.app "$STAGE/Knack.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Knack" -srcfolder "$STAGE" -ov -format UDZO "$DMG_PATH" >/dev/null

if [ -n "$NOTARY_PROFILE" ]; then
  xcrun notarytool submit "$DMG_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG_PATH"
fi

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
    gh release create "$TAG" "$DMG_PATH" "$ZIP_PATH" --verify-tag --title "Knack $VERSION" --notes "$NOTES"
  fi
  if [ -f site/index.html ]; then
    (cd site && vercel deploy --prod --yes)
  fi
fi
