// Renders the DMG window background at 1x and 2x.
// Usage: swift assets/dmg/render-background.swift <out-dir>
import AppKit

// Finder shows ~360pt of content (window height minus title bar); the rest is slack.
let W: CGFloat = 640, H: CGFloat = 400
// Icon centers, in Finder's top-left coordinates. Keep in sync with dmg-settings.py.
let appX: CGFloat = 180, appsX: CGFloat = 460, iconY: CGFloat = 160

func render(scale: CGFloat, to url: URL) {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(W * scale), pixelsHigh: Int(H * scale),
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: W, height: H)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    // Flip to top-left origin to match Finder icon positions.
    ctx.translateBy(x: 0, y: H); ctx.scaleBy(x: 1, y: -1)

    // Canvas: soft vertical wash, near-white to Apple's light gray.
    let wash = NSGradient(colors: [NSColor(srgbRed: 0.992, green: 0.992, blue: 0.996, alpha: 1),
                                   NSColor(srgbRed: 0.937, green: 0.937, blue: 0.950, alpha: 1)])!
    wash.draw(in: NSRect(x: 0, y: 0, width: W, height: H), angle: 90)

    // A faint pool of light behind each icon so they feel set on the page.
    for x in [appX, appsX] {
        let glow = NSGradient(colors: [NSColor(white: 1, alpha: 0.95), NSColor(white: 1, alpha: 0)])!
        glow.draw(fromCenter: NSPoint(x: x, y: iconY), radius: 0,
                  toCenter: NSPoint(x: x, y: iconY), radius: 120, options: [])
    }

    // Arrow: a gentle arc from app to Applications, with a small open chevron.
    let ink = NSColor(srgbRed: 0.525, green: 0.525, blue: 0.545, alpha: 1) // #86868b
    let line = NSColor(srgbRed: 0.631, green: 0.631, blue: 0.651, alpha: 1) // #a1a1a6
    let start = NSPoint(x: appX + 88, y: iconY - 4)
    let end = NSPoint(x: appsX - 92, y: iconY - 4)
    let arc = NSBezierPath()
    arc.move(to: start)
    arc.curve(to: end, controlPoint1: NSPoint(x: start.x + 30, y: iconY - 34),
              controlPoint2: NSPoint(x: end.x - 30, y: iconY - 34))
    arc.lineWidth = 1.75; arc.lineCapStyle = .round
    line.setStroke(); arc.stroke()

    // Chevron aligned with the curve's end tangent (end - controlPoint2).
    let angle = atan2(end.y - (iconY - 34), end.x - (end.x - 30))
    let len: CGFloat = 9, spread: CGFloat = .pi / 5
    let head = NSBezierPath()
    head.move(to: NSPoint(x: end.x - len * cos(angle - spread), y: end.y - len * sin(angle - spread)))
    head.line(to: end)
    head.line(to: NSPoint(x: end.x - len * cos(angle + spread), y: end.y - len * sin(angle + spread)))
    head.lineWidth = 1.75; head.lineCapStyle = .round; head.lineJoinStyle = .round
    head.stroke()

    // Caption.
    let para = NSMutableParagraphStyle(); para.alignment = .center
    let attrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: 13, weight: .regular),
        .foregroundColor: ink, .paragraphStyle: para, .kern: 0.1,
    ]
    // Draw text unflipped.
    ctx.saveGState(); ctx.translateBy(x: 0, y: H); ctx.scaleBy(x: 1, y: -1)
    let caption = NSAttributedString(string: "Drag Knack to Applications", attributes: attrs)
    caption.draw(in: NSRect(x: 0, y: H - 312, width: W, height: 20))
    ctx.restoreGState()

    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: url)
}

let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ".")
render(scale: 1, to: out.appendingPathComponent("background.png"))
render(scale: 2, to: out.appendingPathComponent("background@2x.png"))
