import SwiftUI

/// Right-hand Inspector on iPad / bottom sheet on iPhone showing the spectrum gauge,
/// culling reasons & A-cut strengths, composition notes, non-destructive auto-framing, and EXIF.
struct InspectorView: View {
    let detail: LoupePhotoDetail?
    let threshold: Int
    let onRate: (Int) -> Void
    let onFlag: (Int) -> Void
    let onAutoFrame: (Bool) -> Void
    let onResetFraming: () -> Void
    let onExport: () -> Void
    let onDelete: () -> Void

    @Environment(\.loupePalette) private var palette

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let detail {
                    headerSection(detail)
                    quickActionsSection(detail.summary)
                    cullSection(detail)
                    framingSection(detail)
                    if !detail.compNotes.isEmpty {
                        compositionSection(detail.compNotes)
                    }
                    exifSection(detail)
                } else {
                    Text("사진 정보를 불러오는 중…")
                        .font(.subheadline)
                        .foregroundStyle(palette.text2)
                        .padding(.vertical, 24)
                }
            }
            .padding(16)
        }
        .background(palette.panel)
    }

    private func headerSection(_ detail: LoupePhotoDetail) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text(detail.summary.name)
                    .font(.headline)
                    .foregroundStyle(palette.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                if let badge = detail.summary.formatBadge {
                    Text(badge)
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(palette.control, in: Capsule())
                        .foregroundStyle(palette.text2)
                }
            }
            Text(LoupeFormat.dateTime(detail.summary.capturedMs))
                .font(.caption)
                .foregroundStyle(palette.text2)
                .lineLimit(1)
        }
    }

    private func quickActionsSection(_ summary: LoupePhotoSummary) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            // Stars 1..5 (tap active star to clear)
            HStack(spacing: 6) {
                ForEach(1...5, id: \.self) { star in
                    Button {
                        onRate(summary.rating == star ? 0 : star)
                    } label: {
                        Image(systemName: star <= summary.rating ? "star.fill" : "star")
                            .font(.title3)
                            .foregroundStyle(star <= summary.rating ? palette.star : palette.text3)
                            .frame(minWidth: 32, minHeight: 32)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("별점 \(star)개")
                }
                Spacer()
                Button {
                    onExport()
                } label: {
                    Image(systemName: "square.and.arrow.up")
                        .foregroundStyle(palette.text)
                }
                .buttonStyle(.bordered)
                .tint(palette.control)
                .accessibilityLabel("내보내기")
            }

            HStack(spacing: 8) {
                Button {
                    onFlag(summary.flag > 0 ? 0 : 1)
                } label: {
                    Label("픽", systemImage: "flag.fill")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(summary.flag > 0 ? palette.success : palette.control)
                .foregroundStyle(summary.flag > 0 ? .white : palette.text)

                Button {
                    onFlag(summary.flag < 0 ? 0 : -1)
                } label: {
                    Label("거절", systemImage: "xmark.circle.fill")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(summary.flag < 0 ? palette.danger : palette.control)
                .foregroundStyle(summary.flag < 0 ? .white : palette.text)

                Button(role: .destructive) {
                    onDelete()
                } label: {
                    Image(systemName: "trash")
                }
                .buttonStyle(.bordered)
                .accessibilityLabel("휴지통으로 옮기기")
            }
        }
    }

    private func cullSection(_ detail: LoupePhotoDetail) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("셀렉 도우미")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(palette.text)
                Spacer()
                if !detail.scene.isEmpty {
                    Text(detail.scene)
                        .font(.caption.weight(.medium))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(palette.control, in: Capsule())
                        .foregroundStyle(palette.text2)
                }
            }

            if let bPct = detail.summary.bPct {
                SpectrumGaugeView(bPct: bPct, threshold: threshold)
            } else {
                Text("아직 분석되지 않았습니다")
                    .font(.caption)
                    .foregroundStyle(palette.text3)
            }

            if let topPct = detail.summary.topPct, topPct <= 15 {
                Label("가장 잘 나온 사진 상위 \(topPct)% · A컷 후보", systemImage: "sparkles")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(palette.success)
            }

            if detail.summary.burstSize > 1 {
                Label(
                    "연사 \(detail.summary.burstRank) / \(detail.summary.burstSize)\(detail.summary.burstRank == 1 ? " · 베스트" : "")",
                    systemImage: "square.stack.3d.down.right"
                )
                .font(.caption)
                .foregroundStyle(detail.summary.burstRank == 1 ? palette.success : palette.text2)
            }

            if !detail.reasons.isEmpty {
                chipGroup(title: "B컷 이유", items: detail.reasons, tint: palette.warning)
            }
            if !detail.strengths.isEmpty {
                chipGroup(title: "A컷 근거", items: detail.strengths, tint: palette.success)
            }
        }
        .padding(12)
        .background(palette.content, in: RoundedRectangle(cornerRadius: 12))
    }

    private func chipGroup(title: String, items: [String], tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(palette.text3)
            FlowWrapLayout(spacing: 6) {
                ForEach(items, id: \.self) { item in
                    Text(item)
                        .font(.caption)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(tint.opacity(0.16), in: Capsule())
                        .foregroundStyle(tint)
                }
            }
        }
    }

    private func framingSection(_ detail: LoupePhotoDetail) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("크롭 · 수평 (원본 유지)")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(palette.text)

            if let framing = detail.framing {
                Text(framing)
                    .font(.caption)
                    .foregroundStyle(palette.accent)
            }

            HStack(spacing: 8) {
                Button("자동 수평") { onAutoFrame(false) }
                    .buttonStyle(.bordered)
                    .tint(palette.accent)
                    .disabled(detail.summary.video)

                Button("자동 구도") { onAutoFrame(true) }
                    .buttonStyle(.bordered)
                    .tint(palette.accent)
                    .disabled(detail.summary.video)

                if detail.summary.hasFraming {
                    Button("원래대로") { onResetFraming() }
                        .buttonStyle(.bordered)
                        .tint(palette.text2)
                }
            }
            .font(.caption.weight(.medium))
        }
        .padding(12)
        .background(palette.content, in: RoundedRectangle(cornerRadius: 12))
    }

    private func compositionSection(_ notes: [LoupeCompNote]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("구도 판정")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(palette.text)
            ForEach(notes) { note in
                HStack(alignment: .top, spacing: 8) {
                    Circle()
                        .fill(noteDotColor(note))
                        .frame(width: 7, height: 7)
                        .padding(.top, 5)
                    Text(note.ignored ? "\(note.text) (꺼 둠)" : note.text)
                        .font(.caption)
                        .foregroundStyle(note.ignored ? palette.text3 : palette.text2)
                }
            }
        }
        .padding(12)
        .background(palette.content, in: RoundedRectangle(cornerRadius: 12))
    }

    private func noteDotColor(_ note: LoupeCompNote) -> Color {
        if note.ignored { return palette.text3 }
        switch note.ok {
        case true: return palette.success
        case false: return palette.warning
        case nil: return palette.text3
        }
    }

    private func exifSection(_ detail: LoupePhotoDetail) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("촬영 정보")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(palette.text)
            if !detail.camera.isEmpty {
                metaRow("카메라", detail.camera)
            }
            if !detail.lens.isEmpty {
                metaRow("렌즈", detail.lens)
            }
            if !detail.exposure.isEmpty {
                metaRow("노출", detail.exposure)
            }
            if detail.summary.width > 0 && detail.summary.height > 0 {
                metaRow("해상도", "\(detail.summary.width) × \(detail.summary.height)")
            }
        }
        .padding(12)
        .background(palette.content, in: RoundedRectangle(cornerRadius: 12))
    }

    private func metaRow(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top) {
            Text(label)
                .font(.caption)
                .foregroundStyle(palette.text3)
                .frame(width: 52, alignment: .leading)
            Text(value)
                .font(.caption)
                .foregroundStyle(palette.text)
                .lineLimit(2)
            Spacer()
        }
    }
}

/// Simple wrapping layout for reason/strength chips without external dependencies.
struct FlowWrapLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? 280
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0

        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x + size.width > maxWidth && x > 0 {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
        return CGSize(width: maxWidth, height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0

        for sub in subviews {
            let size = sub.sizeThatFits(.unspecified)
            if x + size.width > bounds.maxX && x > bounds.minX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            sub.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}

#Preview("인스펙터 · 크림") {
    let sample = LoupePhotoDetail(
        summary: LoupePhotoSummary(
            id: 1,
            name: "IMG_0421.CR3",
            capturedMs: 1_763_683_200_000,
            rating: 4,
            flag: 0,
            bPct: 72,
            topPct: nil,
            burstRank: 2,
            burstSize: 4,
            hasFraming: true,
            video: false,
            raw: true,
            jpeg: true,
            width: 6000,
            height: 4000
        ),
        reasons: ["눈 감음 의심", "수평 1.8° 기울어짐"],
        strengths: [],
        scene: "인물",
        framing: "1.8° 돌리고 92% 크기로 자름",
        compNotes: [
            LoupeCompNote(text: "3분할: 눈 위치가 위 1/3 선에 가까움", ok: true, ignored: false),
            LoupeCompNote(text: "수평 1.8° 기울어짐", ok: false, ignored: false),
        ],
        overlay: nil,
        camera: "Canon EOS R6 Mark II",
        lens: "RF 50mm F1.2L USM",
        exposure: "50mm · ƒ1.4 · 1/500 · ISO 200"
    )
    return InspectorView(
        detail: sample,
        threshold: 60,
        onRate: { _ in },
        onFlag: { _ in },
        onAutoFrame: { _ in },
        onResetFraming: {},
        onExport: {},
        onDelete: {}
    )
    .frame(width: 340, height: 760)
    .environment(\.loupePalette, .cream)
}
