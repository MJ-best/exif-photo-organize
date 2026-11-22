import SwiftUI

/// Toggleable (`G`) composition guide overlay drawn cleanly inside the photo's fitted aspect rect.
/// Rules turned off by the user in Settings are never drawn.
struct CompositionOverlayView: View {
    let overlay: LoupeCompositionOverlay
    @Environment(\.loupePalette) private var palette

    var body: some View {
        Canvas { ctx, size in
            let w = size.width
            let h = size.height

            // 1. Rule of thirds grid
            if overlay.showThirds {
                var grid = Path()
                for frac in [1.0 / 3.0, 2.0 / 3.0] {
                    grid.move(to: CGPoint(x: w * frac, y: 0))
                    grid.addLine(to: CGPoint(x: w * frac, y: h))
                    grid.move(to: CGPoint(x: 0, y: h * frac))
                    grid.addLine(to: CGPoint(x: w, y: h * frac))
                }
                ctx.stroke(grid, with: .color(.white.opacity(0.45)), style: StrokeStyle(lineWidth: 1, dash: [4, 4]))
            }

            // 2. Subject box & key point (eyes / center of mass)
            if overlay.hasSubject {
                let rect = CGRect(
                    x: CGFloat(overlay.subjectX) * w,
                    y: CGFloat(overlay.subjectY) * h,
                    width: CGFloat(overlay.subjectW) * w,
                    height: CGFloat(overlay.subjectH) * h
                )
                ctx.stroke(Path(rect), with: .color(palette.success.opacity(0.8)), lineWidth: 1.5)

                let kx = CGFloat(overlay.keyX) * w
                let ky = CGFloat(overlay.keyY) * h
                let dot = Path(ellipseIn: CGRect(x: kx - 5, y: ky - 5, width: 10, height: 10))
                ctx.fill(dot, with: .color(palette.accent))
                ctx.stroke(dot, with: .color(.white), lineWidth: 1.5)

                // Lead room direction vector
                if overlay.showLeadRoom && abs(overlay.facing) >= 0.2 {
                    let dir: CGFloat = overlay.facing > 0 ? 1 : -1
                    let len = min(w * 0.14, 70) * CGFloat(min(1.0, abs(overlay.facing)))
                    var arrow = Path()
                    arrow.move(to: CGPoint(x: kx, y: ky))
                    arrow.addLine(to: CGPoint(x: kx + dir * len, y: ky))
                    ctx.stroke(arrow, with: .color(palette.accent), lineWidth: 2)
                }
            }

            // 3. Headroom marker
            if overlay.showHeadroom, let headTop = overlay.headTop {
                let y = CGFloat(headTop) * h
                var line = Path()
                line.move(to: CGPoint(x: w * 0.25, y: y))
                line.addLine(to: CGPoint(x: w * 0.75, y: y))
                ctx.stroke(line, with: .color(palette.star.opacity(0.85)), style: StrokeStyle(lineWidth: 1.5, dash: [6, 3]))
            }

            // 4. Background mergers (pole behind head / horizontal neck line)
            if overlay.showMerger {
                if overlay.pole >= 0.45 {
                    let x = CGFloat(overlay.poleX) * w
                    let yEnd = CGFloat(overlay.headTop ?? overlay.keyY) * h
                    var polePath = Path()
                    polePath.move(to: CGPoint(x: x, y: 0))
                    polePath.addLine(to: CGPoint(x: x, y: yEnd))
                    ctx.stroke(polePath, with: .color(palette.danger), lineWidth: 2)
                }
                if overlay.neckLine >= 0.45 {
                    let y = CGFloat(overlay.neckY) * h
                    var neckPath = Path()
                    neckPath.move(to: CGPoint(x: 0, y: y))
                    neckPath.addLine(to: CGPoint(x: w, y: y))
                    ctx.stroke(neckPath, with: .color(palette.danger.opacity(0.85)), style: StrokeStyle(lineWidth: 1.5, dash: [5, 3]))
                }
            }

            // 5. Detected horizon / vertical tilt line
            if overlay.showTilt && overlay.tiltConf >= 0.3 && abs(overlay.tilt) >= 0.4 {
                let rad = CGFloat(overlay.tilt) * .pi / 180.0
                var tiltPath = Path()
                if overlay.tiltVertical {
                    let cx = CGFloat(overlay.tiltAt) * w
                    let dx = tan(rad) * (h * 0.35)
                    tiltPath.move(to: CGPoint(x: cx - dx, y: h * 0.15))
                    tiltPath.addLine(to: CGPoint(x: cx + dx, y: h * 0.85))
                } else {
                    let cy = CGFloat(overlay.tiltAt) * h
                    let dy = tan(rad) * (w * 0.35)
                    tiltPath.move(to: CGPoint(x: w * 0.15, y: cy - dy))
                    tiltPath.addLine(to: CGPoint(x: w * 0.85, y: cy + dy))
                }
                ctx.stroke(tiltPath, with: .color(palette.warning), lineWidth: 2)
            }
        }
        .allowsHitTesting(false)
    }
}
