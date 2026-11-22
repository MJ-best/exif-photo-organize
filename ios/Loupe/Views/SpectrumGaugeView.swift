import SwiftUI

/// LOUPE v0.7.4 bidirectional spectrum gauge:
/// Left = B-cut likelihood (`← B컷 XX%`), Right = A-cut likelihood (`A컷 YY% →`),
/// anchored at the user's B-cut candidate threshold (`1 - threshold / 100`).
struct SpectrumGaugeView: View {
    let bPct: Int
    let threshold: Int
    @Environment(\.loupePalette) private var palette

    private var clampedB: Int { max(0, min(100, bPct)) }
    private var aPct: Int { 100 - clampedB }
    private var position: Double { Double(aPct) / 100.0 }
    private var cutLine: Double { 1.0 - Double(max(30, min(90, threshold))) / 100.0 }
    private var isBCandidate: Bool { clampedB >= threshold }
    private var isStrongA: Bool { clampedB < 40 }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("← B컷 \(clampedB)%")
                    .font(.caption.weight(isBCandidate ? .bold : .medium))
                    .foregroundStyle(isBCandidate ? palette.warning : palette.text2)
                Spacer()
                Text("A컷 \(aPct)% →")
                    .font(.caption.weight(isStrongA ? .bold : .medium))
                    .foregroundStyle(isStrongA ? palette.success : palette.text2)
            }

            GeometryReader { geo in
                let width = max(1, geo.size.width)
                let height: CGFloat = 10
                let cutX = width * cutLine
                let markerX = width * position
                let fillStart = min(cutX, markerX)
                let fillWidth = max(2, abs(markerX - cutX))

                ZStack(alignment: .leading) {
                    // Subtle spectrum track: orange (B) -> neutral -> green (A)
                    Capsule()
                        .fill(
                            LinearGradient(
                                colors: [
                                    palette.warning.opacity(0.24),
                                    palette.control.opacity(0.75),
                                    palette.success.opacity(0.24),
                                ],
                                startPoint: .leading,
                                endPoint: .trailing
                            )
                        )
                        .frame(height: height)

                    // Directional fill from threshold line toward current marker
                    RoundedRectangle(cornerRadius: height / 2)
                        .fill(isBCandidate ? palette.warning : (isStrongA ? palette.success : palette.text2))
                        .frame(width: fillWidth, height: height)
                        .offset(x: fillStart)

                    // Candidate threshold divider line
                    Rectangle()
                        .fill(palette.text.opacity(0.55))
                        .frame(width: 1.5, height: 14)
                        .offset(x: max(0, min(width - 1.5, cutX - 0.75)))

                    // Position dot
                    Circle()
                        .fill(isBCandidate ? palette.warning : (isStrongA ? palette.success : palette.accent))
                        .overlay(Circle().stroke(palette.panel, lineWidth: 2))
                        .frame(width: 14, height: 14)
                        .offset(x: max(0, min(width - 14, markerX - 7)))
                }
            }
            .frame(height: 16)

            Text("기준선: B컷 \(threshold)% 이상일 때 B컷 후보")
                .font(.caption2)
                .foregroundStyle(palette.text3)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("셀렉 도우미 판정: B컷 가능성 \(clampedB)퍼센트, A컷 가능성 \(aPct)퍼센트")
    }
}

#Preview("스펙트럼 게이지 · 크림") {
    VStack(spacing: 20) {
        SpectrumGaugeView(bPct: 92, threshold: 60)
        SpectrumGaugeView(bPct: 48, threshold: 60)
        SpectrumGaugeView(bPct: 12, threshold: 60)
    }
    .padding()
    .background(LoupePalette.cream.panel)
    .environment(\.loupePalette, .cream)
}

#Preview("스펙트럼 게이지 · 코코아") {
    VStack(spacing: 20) {
        SpectrumGaugeView(bPct: 92, threshold: 60)
        SpectrumGaugeView(bPct: 12, threshold: 60)
    }
    .padding()
    .background(LoupePalette.cocoa.panel)
    .environment(\.loupePalette, .cocoa)
    .preferredColorScheme(.dark)
}
