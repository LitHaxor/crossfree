// Draws crossfree's app icon: a cross (an X of two woven strokes) whose
// rising stroke is also the stem of an F, on a dark squircle.
//
//   swift tools/make-icon.swift OUT_DIR [xf|cross]
//   iconutil -c icns OUT_DIR/AppIcon.iconset -o app/Resources/AppIcon.icns
//
// Writes OUT_DIR/AppIcon.iconset (every size macOS wants) and OUT_DIR/icon.png
// (1024 px). `xf`, the app's icon, makes the rising stroke an F's stem;
// `cross` is the plain X.
import AppKit

let style = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "xf"

func color(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: r, green: g, blue: b, alpha: a)
}

/// A rounded bar from `a` to `b`, `width` thick.
func bar(_ a: CGPoint, _ b: CGPoint, _ width: CGFloat) -> CGPath {
    let path = CGMutablePath()
    path.move(to: a)
    path.addLine(to: b)
    return path.copy(strokingWithWidth: width, lineCap: .round, lineJoin: .round, miterLimit: 4)
}

/// Fills `path` with a linear gradient from `from` (at `p0`) to `to` (at `p1`).
func fill(_ ctx: CGContext, _ path: CGPath, _ from: CGColor, _ to: CGColor, _ p0: CGPoint, _ p1: CGPoint) {
    ctx.saveGState()
    ctx.addPath(path)
    ctx.clip()
    let gradient = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [from, to] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(gradient, start: p0, end: p1, options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
    ctx.restoreGState()
}

/// Draws the icon in a 1024-unit square (y up).
func draw(_ ctx: CGContext) {
    let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
    let shape = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)

    // The tile: deep navy, lit from the middle, a soft shadow under it.
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: color(0, 0, 0, 0.45))
    ctx.addPath(shape)
    ctx.setFillColor(color(0.05, 0.06, 0.12))
    ctx.fillPath()
    ctx.restoreGState()
    fill(ctx, shape, color(0.11, 0.14, 0.27), color(0.03, 0.04, 0.09), CGPoint(x: 512, y: 924), CGPoint(x: 512, y: 100))
    ctx.saveGState()
    ctx.addPath(shape)
    ctx.clip()
    let glow = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB),
        colors: [color(0.35, 0.62, 1, 0.42), color(0.35, 0.62, 1, 0)] as CFArray, locations: [0, 1])!
    ctx.drawRadialGradient(
        glow, startCenter: CGPoint(x: 512, y: 540), startRadius: 0,
        endCenter: CGPoint(x: 512, y: 540), endRadius: 430, options: [])
    ctx.restoreGState()
    ctx.addPath(shape)
    ctx.setStrokeColor(color(1, 1, 1, 0.09))
    ctx.setLineWidth(3)
    ctx.strokePath()

    // The cross. `fall` runs top-left to bottom-right; `rise` bottom-left to
    // top-right, over it. In the XF style the X sits left, and the rising
    // stroke is the slanted stem of an F.
    let width: CGFloat = style == "xf" ? 122 : 132
    let shift: CGFloat = style == "xf" ? -62 : 0
    let fallA = CGPoint(x: 300 + shift, y: 760)
    let fallB = CGPoint(x: 724 + shift, y: 264)
    let riseA = CGPoint(x: 300 + shift, y: 264)
    let riseB = CGPoint(x: 724 + shift, y: 760)
    let fall = bar(fallA, fallB, width)
    let rise = bar(riseA, riseB, width)

    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 24, color: color(0, 0, 0, 0.5))
    fill(ctx, fall, color(1, 1, 1), color(0.78, 0.82, 0.92), fallA, fallB)
    ctx.restoreGState()

    var arms: [CGPath] = []
    if style == "xf" {
        // F's arms from the top of its stem and a third of the way down.
        let top = riseB
        let t: CGFloat = 0.4
        let middle = CGPoint(x: riseB.x + (riseA.x - riseB.x) * t, y: riseB.y + (riseA.y - riseB.y) * t)
        arms = [
            bar(top, CGPoint(x: top.x + 170, y: top.y), width * 0.8),
            bar(middle, CGPoint(x: middle.x + 130, y: middle.y), width * 0.8),
        ]
    }
    // A gap of the tile's colour around the rising stroke weaves it over
    // the falling one (the F's arms cross nothing, so they get none).
    let over = CGMutablePath()
    over.addPath(rise)
    for arm in arms { over.addPath(arm) }
    ctx.saveGState()
    ctx.addPath(rise)
    ctx.setStrokeColor(color(0.06, 0.08, 0.16))
    ctx.setLineWidth(30)
    ctx.setLineJoin(.round)
    ctx.strokePath()
    ctx.restoreGState()
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10), blur: 30, color: color(0.2, 0.5, 1, 0.55))
    ctx.beginTransparencyLayer(auxiliaryInfo: nil)
    for part in [rise] + arms {
        fill(ctx, part, color(0.45, 0.85, 1), color(0.32, 0.36, 1), riseA, CGPoint(x: riseB.x + 170, y: riseB.y))
    }
    ctx.endTransparencyLayer()
    ctx.restoreGState()
}

func png(_ size: Int) -> Data {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)!
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!.cgContext
    ctx.scaleBy(x: CGFloat(size) / 1024, y: CGFloat(size) / 1024)
    draw(ctx)
    return rep.representation(using: .png, properties: [:])!
}

let out = URL(fileURLWithPath: CommandLine.arguments[1])
let set = out.appendingPathComponent("AppIcon.iconset")
try? FileManager.default.removeItem(at: set)
try FileManager.default.createDirectory(at: set, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    try png(points).write(to: set.appendingPathComponent("icon_\(points)x\(points).png"))
    try png(points * 2).write(to: set.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
}
try png(1024).write(to: out.appendingPathComponent("icon.png"))
