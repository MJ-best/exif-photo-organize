import SwiftUI
import UIKit

/// Full-screen culling viewer for iPad & iPhone:
/// - Keeps zoom scale & pan position across photo navigation (v0.6.1)
/// - Double-Space (within 0.3s) or `Z` toggles 2.5x center zoom; single Space advances
/// - Full desktop hardware keyboard shortcuts on iPad (`1..5`, `0`, `P`, `X`, `U`, `Z`, `R`, `G`, `←/→`, `Delete`, `Esc`)
/// - Non-intrusive bottom-strip notifications (never covers the center of the photo — DEVLOG §3)
struct ViewerView: View {
    @Bindable var store: LibraryStore
    let session: ViewerSession

    @State private var scale: CGFloat = 1.0
    @State private var offset: CGSize = .zero
    @State private var lastSpaceDate: Date = .distantPast
    @State private var phoneInspectorPresented: Bool = false

    @Environment(\.loupePalette) private var palette
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.displayScale) private var displayScale

    private var currentID: Int64? { session.currentID }
    private var detail: LoupePhotoDetail? { store.activeDetail }

    var body: some View {
        GeometryReader { geo in
            let isWide = sizeClass == .regular && geo.size.width >= 820
            let previewEdge = min(3200, max(1280, Int(max(geo.size.width, geo.size.height) * displayScale)))

            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    topBar(isWide: isWide)

                    ZStack {
                        if let id = currentID {
                            ZoomablePhotoView(
                                photoID: id,
                                maxEdge: previewEdge,
                                reloadToken: store.imageReloadToken,
                                pipeline: store.images,
                                overlay: detail?.overlay,
                                showOverlay: store.showCompositionOverlay,
                                scale: $scale,
                                offset: $offset,
                                onSwipeUpPick: { toggleFlag(1) },
                                onSwipeDownReject: { toggleFlag(-1) },
                                onSwipeLeftNext: { step(1, edge: previewEdge) },
                                onSwipeRightPrev: { step(-1, edge: previewEdge) }
                            )
                        }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                    // Compact scrollable filmstrip strictly BELOW photo (hidden in ultra-short landscape)
                    if session.ids.count > 1 && geo.size.height >= 480 {
                        FilmstripBarView(
                            ids: session.ids,
                            currentIndex: session.index,
                            summaries: store.summaries,
                            reloadToken: store.imageReloadToken,
                            pipeline: store.images
                        ) { nextIndex in
                            store.viewerMoved(to: nextIndex, previewEdge: previewEdge)
                        }
                    }

                    // Bottom notification & quick-cull strip (strictly BELOW photo, never covering it)
                    bottomStrip(isWide: isWide, previewEdge: previewEdge)
                }

                if isWide && store.showInspector {
                    Divider().overlay(palette.separator)
                    InspectorView(
                        detail: detail,
                        threshold: store.threshold,
                        onRate: { rateCurrent($0) },
                        onFlag: { toggleFlag($0) },
                        onAutoFrame: { compose in
                            if let id = currentID { store.autoFrame(id: id, compose: compose) }
                        },
                        onResetFraming: {
                            if let id = currentID { store.resetFraming(id: id) }
                        },
                        onExport: {
                            if let id = currentID { store.openExport(for: [id]) }
                        },
                        onDelete: {
                            if let id = currentID { store.askDelete(ids: [id]) }
                        }
                    )
                    .frame(width: 340)
                }
            }
            .background(palette.viewer)
            .onAppear {
                if currentID != nil {
                    store.images.prefetchNeighbors(ids: session.ids, around: session.index, maxEdge: previewEdge)
                }
            }
            .sheet(isPresented: $phoneInspectorPresented) {
                InspectorView(
                    detail: detail,
                    threshold: store.threshold,
                    onRate: { rateCurrent($0) },
                    onFlag: { toggleFlag($0) },
                    onAutoFrame: { compose in
                        if let id = currentID { store.autoFrame(id: id, compose: compose) }
                    },
                    onResetFraming: {
                        if let id = currentID { store.resetFraming(id: id) }
                    },
                    onExport: {
                        phoneInspectorPresented = false
                        if let id = currentID { store.openExport(for: [id]) }
                    },
                    onDelete: {
                        phoneInspectorPresented = false
                        if let id = currentID { store.askDelete(ids: [id]) }
                    }
                )
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
            }
            // Hardware keyboard shortcuts (iPad Magic Keyboard / Bluetooth keyboards)
            .background(keyboardShortcutsLayer(previewEdge: previewEdge))
        }
    }

    private func topBar(isWide: Bool) -> some View {
        HStack(spacing: 10) {
            Button {
                store.closeViewer()
            } label: {
                if isWide {
                    Label("돌아가기", systemImage: "chevron.left")
                } else {
                    Image(systemName: "chevron.left")
                        .frame(minWidth: 28, minHeight: 28)
                }
            }
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(palette.text)
            .buttonStyle(.plain)
            .accessibilityLabel("돌아가기")

            // Title yields remaining space so tools on the right never collide (DEVLOG §3)
            VStack(alignment: .leading, spacing: 1) {
                Text(detail?.summary.name ?? "")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(palette.text)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 6) {
                    if let ms = detail?.summary.capturedMs {
                        Text(isWide ? LoupeFormat.dateTime(ms) : LoupeFormat.compactDate(ms))
                            .font(.caption2)
                            .foregroundStyle(palette.text2)
                            .lineLimit(1)
                    }
                    Text("\(session.index + 1) / \(session.ids.count)")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(palette.text3)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            // Fixed right-side tools laid out first in priority
            HStack(spacing: 8) {
                if store.canUndo {
                    Button {
                        LoupeHaptics.selection()
                        store.undoLastDecision()
                    } label: {
                        Image(systemName: "arrow.uturn.backward")
                            .foregroundStyle(palette.text)
                    }
                    .buttonStyle(.bordered)
                    .tint(palette.control)
                    .accessibilityLabel("실행 취소")
                }

                Button {
                    toggleCenterZoom()
                } label: {
                    Image(systemName: scale > 1.05 ? "minus.magnifyingglass" : "plus.magnifyingglass")
                        .foregroundStyle(scale > 1.05 ? palette.accent : palette.text)
                }
                .buttonStyle(.bordered)
                .tint(palette.control)
                .accessibilityLabel("100% 확대 전환")

                Button {
                    store.showCompositionOverlay.toggle()
                } label: {
                    Image(systemName: "grid")
                        .foregroundStyle(store.showCompositionOverlay ? palette.accent : palette.text)
                }
                .buttonStyle(.bordered)
                .tint(palette.control)
                .accessibilityLabel("구도 표시")

                Button {
                    if isWide {
                        withAnimation(.easeInOut(duration: 0.18)) { store.showInspector.toggle() }
                    } else {
                        phoneInspectorPresented = true
                    }
                } label: {
                    Image(systemName: "info.circle")
                        .foregroundStyle((isWide && store.showInspector) ? palette.accent : palette.text)
                }
                .buttonStyle(.bordered)
                .tint(palette.control)
                .accessibilityLabel("정보 패널")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(palette.toolbar)
        .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }
    }

    private func bottomStrip(isWide: Bool, previewEdge: Int) -> some View {
        VStack(spacing: 0) {
            Divider().overlay(palette.separator)
            HStack(spacing: 10) {
                Button {
                    step(-1, edge: previewEdge)
                } label: {
                    Image(systemName: "chevron.left")
                }
                .disabled(session.index <= 0)
                .buttonStyle(.bordered)
                .tint(palette.control)

                if let toast = store.toast {
                    HStack(spacing: 8) {
                        Text(toast.text)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.white)
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 5)
                            .background(toastColor(toast.tone), in: Capsule())

                        if toast.undoAvailable && store.canUndo {
                            Button("되돌리기") {
                                LoupeHaptics.selection()
                                store.undoLastDecision()
                            }
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(palette.accent)
                            .buttonStyle(.plain)
                        }
                    }
                    .transition(.opacity)
                } else if !isWide, let summary = detail?.summary {
                    HStack(spacing: 8) {
                        Button {
                            toggleFlag(1)
                        } label: {
                            Image(systemName: "flag.fill")
                                .foregroundStyle(summary.flag > 0 ? palette.success : palette.text2)
                        }
                        Button {
                            toggleFlag(-1)
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(summary.flag < 0 ? palette.danger : palette.text2)
                        }
                        if let bPct = summary.bPct {
                            Text(bPct >= store.threshold ? "B컷 \(bPct)%" : "A컷 \(100 - bPct)%")
                                .font(.caption.weight(.semibold).monospacedDigit())
                                .foregroundStyle(bPct >= store.threshold ? palette.warning : palette.success)
                                .lineLimit(1)
                        }
                    }
                } else {
                    Text("위로 밀어 픽 · 아래로 밀어 거절 · 두 번 탭 확대 · ⌘Z 되돌리기")
                        .font(.caption)
                        .foregroundStyle(palette.text3)
                        .lineLimit(1)
                }

                Spacer(minLength: 0)

                if scale > 1.05 {
                    Text("확대 유지 중 (\(String(format: "%.1fx", scale)))")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(palette.accent)
                        .lineLimit(1)
                }

                Button {
                    step(1, edge: previewEdge)
                } label: {
                    Image(systemName: "chevron.right")
                }
                .disabled(session.index >= session.ids.count - 1)
                .buttonStyle(.bordered)
                .tint(palette.control)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .frame(minHeight: 44)
            .background(palette.toolbar)
        }
    }

    @ViewBuilder
    private func keyboardShortcutsLayer(previewEdge: Int) -> some View {
        HardwareKeyHandler(
            callbacks: .init(
                onRate: { rateCurrent($0) },
                onPick: { toggleFlag(1) },
                onReject: { toggleFlag(-1) },
                onClearFlag: { setFlagExplicit(0) },
                onToggleZoom: { toggleCenterZoom() },
                onToggleOverlay: { store.showCompositionOverlay.toggle() },
                onAutoStraighten: {
                    if let id = currentID { store.autoFrame(id: id, compose: false) }
                },
                onPrevious: { step(-1, edge: previewEdge) },
                onNext: { step(1, edge: previewEdge) },
                onSpace: { handleSpaceShortcut(edge: previewEdge) },
                onUndo: {
                    LoupeHaptics.selection()
                    store.undoLastDecision()
                },
                onDelete: {
                    if let id = currentID { store.askDelete(ids: [id]) }
                },
                onClose: { store.closeViewer() }
            )
        )
        .frame(width: 0, height: 0)
        .accessibilityHidden(true)
    }

    private func handleSpaceShortcut(edge: Int) {
        let now = Date()
        if now.timeIntervalSince(lastSpaceDate) <= 0.3 {
            lastSpaceDate = .distantPast
            toggleCenterZoom()
        } else {
            lastSpaceDate = now
            step(1, edge: edge)
        }
    }

    private func toggleCenterZoom() {
        LoupeHaptics.selection()
        withAnimation(.easeInOut(duration: 0.18)) {
            if scale > 1.05 {
                scale = 1.0
                offset = .zero
            } else {
                scale = 2.5
                offset = .zero
            }
        }
    }

    private func step(_ delta: Int, edge: Int) {
        let next = max(0, min(session.ids.count - 1, session.index + delta))
        guard next != session.index else { return }
        store.viewerMoved(to: next, previewEdge: edge)
    }

    private func rateCurrent(_ rating: Int) {
        guard let id = currentID else { return }
        LoupeHaptics.selection()
        store.rate(ids: [id], rating: rating)
    }

    private func toggleFlag(_ target: Int) {
        guard let id = currentID else { return }
        LoupeHaptics.impact()
        let currentFlag = detail?.summary.flag ?? store.summaries[id]?.flag ?? 0
        let next = (currentFlag == target) ? 0 : target
        store.flag(ids: [id], flag: next)
    }

    private func setFlagExplicit(_ flag: Int) {
        guard let id = currentID else { return }
        LoupeHaptics.selection()
        store.flag(ids: [id], flag: flag)
    }

    private func toastColor(_ tone: BottomToast.Tone) -> Color {
        switch tone {
        case .positive: return palette.success
        case .negative: return palette.danger
        case .accent: return palette.accent
        case .neutral: return palette.hud
        }
    }
}

@MainActor
private enum LoupeHaptics {
    private static let selectionGenerator = UISelectionFeedbackGenerator()
    private static let impactGenerator = UIImpactFeedbackGenerator(style: .medium)

    static func selection() {
        selectionGenerator.selectionChanged()
    }

    static func impact() {
        impactGenerator.impactOccurred()
    }
}
