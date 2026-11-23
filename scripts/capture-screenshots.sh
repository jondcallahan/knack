#!/bin/sh
# Captures clean Light & Dark Retina window screenshots of Knack using the
# synthetic demo library (never real user skills) and generates site icons.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_BIN="/Applications/Knack.app/Contents/MacOS/knack"
DEMO_ROOT="$(mktemp -d /tmp/knack-demo.XXXXXX)"
DEMO_HOME="$DEMO_ROOT/home"
OUT_DIR="$ROOT/site/img"

FIND_WID="$DEMO_ROOT/knack-find-wid"
trap 'rm -rf "$DEMO_ROOT"' EXIT

if [ ! -x "$APP_BIN" ]; then
  echo "Expected built app at $APP_BIN; run ./bundle.sh first." >&2
  exit 1
fi

"$ROOT/scripts/seed-demo.sh" "$DEMO_HOME"
mkdir -p "$OUT_DIR"

# Generate crisp web icons from assets/icon.png
sips -z 256 256 "$ROOT/assets/icon.png" --out "$OUT_DIR/icon-256.png" >/dev/null
sips -z 64 64 "$ROOT/assets/icon.png" --out "$ROOT/site/favicon.png" >/dev/null
sips -z 180 180 "$ROOT/assets/icon.png" --out "$ROOT/site/apple-touch-icon.png" >/dev/null

# Helper returning the window ID owned by a PID via CoreGraphics (compiled once).
cat > "$DEMO_ROOT/knack-find-wid.swift" <<'SWIFT'
import Cocoa
import CoreGraphics

guard CommandLine.arguments.count > 1, let pid = Int32(CommandLine.arguments[1]) else {
    exit(1)
}
let opts: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
if let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] {
    for w in list {
        guard let owner = w[kCGWindowOwnerPID as String] as? Int32, owner == pid,
              let bounds = w[kCGWindowBounds as String] as? [String: Any],
              let width = bounds["Width"] as? Double, width >= 800,
              let wid = w[kCGWindowNumber as String] as? Int else { continue }
        print(wid)
        exit(0)
    }
}
exit(1)
SWIFT
swiftc -O "$DEMO_ROOT/knack-find-wid.swift" -o "$FIND_WID"

capture_shot() {
  out_path="$1"
  appearance="$2"
  start_mode="${3:-}"

  rm -rf "$DEMO_HOME/.config/knack"
  mkdir -p "$DEMO_HOME/.config/knack"
  cat > "$DEMO_HOME/.config/knack/prefs.json" <<'JSON'
{
  "folders": ["~/dev"],
  "scope": "all",
  "status": "all",
  "selected": null,
  "new_dirs": ["~/.agents/skills", "~/.claude/skills"],
  "side_width": 236.0,
  "list_width": 320.0
}
JSON

  KNACK_NO_UPDATE_CHECK=1 \
  KNACK_HOME="$DEMO_HOME" \
  KNACK_PREFS="$DEMO_HOME/.config/knack/prefs.json" \
  KNACK_SELECT="pr-review-checklist" \
  KNACK_APPEARANCE="$appearance" \
  KNACK_START_MODE="$start_mode" \
    "$APP_BIN" >/dev/null 2>&1 &
  pid=$!

  wid=""
  i=0
  while [ $i -lt 40 ]; do
    if wid="$("$FIND_WID" "$pid" 2>/dev/null)"; then
      break
    fi
    sleep 0.15
    i=$((i + 1))
  done

  if [ -z "$wid" ]; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "Timed out waiting for Knack window ($out_path)" >&2
    exit 1
  fi

  # Let initial scan and syntax highlighting finish rendering.
  sleep 1.0
  /usr/sbin/screencapture -l "$wid" -o -x "$out_path"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "Captured $out_path"
}

capture_shot "$OUT_DIR/knack-light.png" "light" ""
capture_shot "$OUT_DIR/knack-dark.png" "dark" ""
capture_shot "$OUT_DIR/knack-edit-light.png" "light" "edit"
capture_shot "$OUT_DIR/knack-edit-dark.png" "dark" "edit"
capture_shot "$OUT_DIR/knack-new-light.png" "light" "new"
capture_shot "$OUT_DIR/knack-new-dark.png" "dark" "new"

# Generate 1200x630 OpenGraph preview card framing the Light hero window cleanly.
cat > "$DEMO_ROOT/knack-make-og.swift" <<'SWIFT'
import Cocoa

let srcPath = CommandLine.arguments[1]
let dstPath = CommandLine.arguments[2]
guard let src = NSImage(contentsOfFile: srcPath) else { exit(1) }

let W: CGFloat = 1200
let H: CGFloat = 630
let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: Int(W),
    pixelsHigh: Int(H),
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
)!

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
let ctx = NSGraphicsContext.current!.cgContext

// Soft Apple-style light backdrop
ctx.setFillColor(NSColor(calibratedWhite: 0.96, alpha: 1.0).cgColor)
ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))

let padX: CGFloat = 64
let winW = W - padX * 2
let aspect = src.size.height / max(src.size.width, 1)
let winH = winW * aspect
// Align top of window near top margin (AppKit y=0 is bottom).
let rect = CGRect(x: padX, y: H - 36 - winH, width: winW, height: winH)

ctx.saveGState()
let shadow = NSShadow()
shadow.shadowBlurRadius = 28
shadow.shadowOffset = NSSize(width: 0, height: -10)
shadow.shadowColor = NSColor(calibratedWhite: 0, alpha: 0.16)
shadow.set()
let path = NSBezierPath(roundedRect: rect, xRadius: 12, yRadius: 12)
NSColor.white.setFill()
path.fill()
ctx.restoreGState()

ctx.saveGState()
NSBezierPath(roundedRect: rect, xRadius: 12, yRadius: 12).addClip()
src.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
ctx.restoreGState()

NSGraphicsContext.restoreGraphicsState()

if let data = rep.representation(using: .png, properties: [:]) {
    try data.write(to: URL(fileURLWithPath: dstPath))
}
SWIFT
swift "$DEMO_ROOT/knack-make-og.swift" "$OUT_DIR/knack-light.png" "$OUT_DIR/og.png"
