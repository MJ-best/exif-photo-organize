import SwiftUI

/// Windowed lazy photo grid with contextual B-cut/A-cut/Rejected action bars, multi-select
/// culling bar, thermal-aware analysis progress strip, and non-intrusive bottom toast strip.
struct LibraryGridView: View {
    @Bindable var store: LibraryStore
    @Environment(\.loupePalette) private var palette
    @Environment(\.horizontalSizeClass) private var sizeClass

    private var columns: [GridItem] {
        let minSize: CGFloat = sizeClass == .regular ? 156 : 112
        return [GridItem(.adaptive(minimum: minSize, maximum: 220), spacing: 6)]
    }

    var body: some View {
        VStack(spacing: 0) {
            if sizeClass == .compact {
                compactFilterRibbon
            }

            if let progress = store.analysisProgress {
                analysisBanner(progress)
            }

            contextualBar

            if !store.isReady {
                loadingPlaceholder
            } else if store.visibleIDs.isEmpty {
                emptyState
            } else {
                ScrollView {
                    LazyVGrid(columns: columns, spacing: 6) {
                        ForEach(store.visibleIDs, id: \.self) { id in
                            PhotoCellView(
                                photoID: id,
                                summary: store.summaries[id],
                                selected: store.selectedIDs.contains(id),
                                isSelectionMode: store.isSelectionMode,
                                threshold: store.threshold,
                                reloadToken: store.imageReloadToken,
                                pipeline: store.images
                            )
                            .onAppear {
                                store.ensureLoaded(for: id)
                            }
                            .onTapGesture {
                                if store.isSelectionMode {
                                    store.toggleSelection(id: id)
                                } else {
                                    store.openViewer(at: id)
                                }
                            }
                            .onLongPressGesture {
                                store.isSelectionMode = true
                                store.toggleSelection(id: id)
                            }
                        }
                    }
                    .padding(8)
                }
            }

            bottomBar
        }
        .background(palette.content)
        .navigationTitle(store.activeView.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .topBarTrailing) {
                if !store.visibleIDs.isEmpty {
                    Button(store.isSelectionMode ? "완료" : "선택") {
                        if store.isSelectionMode {
                            store.clearSelection()
                        } else {
                            store.isSelectionMode = true
                        }
                    }
                }
                Button {
                    store.openImport()
                } label: {
                    Image(systemName: "square.and.arrow.down")
                }
                .accessibilityLabel("SD 카드 / 폴더 가져오기")

                if sizeClass == .compact {
                    Button {
                        store.refreshTrashBytes()
                        store.settingsSheetPresented = true
                    } label: {
                        Image(systemName: "gearshape")
                    }
                    .accessibilityLabel("설정")
                }
            }
        }
    }

    private var compactFilterRibbon: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(LoupeViewFilter.allCases) { filter in
                    let active = store.activeView == filter
                    let count = store.counts.count(for: filter)
                    Button {
                        store.selectView(filter)
                    } label: {
                        HStack(spacing: 4) {
                            Text(filter.shortTitle)
                            if count > 0 {
                                Text("\(count)")
                                    .font(.caption2.monospacedDigit())
                                    .opacity(0.8)
                            }
                        }
                        .font(.caption.weight(active ? .semibold : .medium))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(active ? palette.accent : palette.control, in: Capsule())
                        .foregroundStyle(active ? .white : palette.text)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .background(palette.toolbar)
        .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }
    }

    private func analysisBanner(_ progress: AnalysisProgress) -> some View {
        HStack(spacing: 10) {
            if progress.pausedByThermal {
                Image(systemName: "thermometer.sun.fill")
                    .foregroundStyle(palette.warning)
                Text("기기 온도가 높아 셀렉 분석을 잠시 멈췄습니다")
                    .font(.caption)
                    .foregroundStyle(palette.text)
                    .lineLimit(1)
            } else {
                ProgressView()
                    .controlSize(.small)
                    .tint(palette.accent)
                Text("셀렉 도우미 분석 중 (\(progress.done) / \(progress.total))")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(palette.text)
                    .lineLimit(1)
                if let notice = store.thermalMonitor.notice {
                    Text("· \(notice)")
                        .font(.caption2)
                        .foregroundStyle(palette.text2)
                        .lineLimit(1)
                }
            }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(palette.selected.opacity(0.7))
        .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }
    }

    @ViewBuilder
    private var contextualBar: some View {
        switch store.activeView {
        case .candidates:
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 10) {
                    Text(store.learn.summary)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(palette.text2)
                        .lineLimit(1)
                    Spacer()
                    if !store.visibleIDs.isEmpty {
                        Button("\(store.visibleIDs.count)장 모두 거절") {
                            store.flag(ids: store.visibleIDs, flag: -1)
                        }
                        .font(.caption.weight(.semibold))
                        .buttonStyle(.borderedProminent)
                        .tint(palette.danger)
                    }
                }
                HStack(spacing: 8) {
                    Text("B컷 기준 \(store.threshold)%")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(palette.text2)
                        .frame(width: 84, alignment: .leading)
                    Slider(
                        value: Binding(
                            get: { Double(store.threshold) },
                            set: { store.updateThreshold(Int($0.rounded())) }
                        ),
                        in: 30...90,
                        step: 5
                    )
                    .tint(palette.warning)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(palette.toolbar)
            .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }

        case .keepers:
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 10) {
                    Text(store.learn.summary)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(palette.text2)
                        .lineLimit(1)
                    Spacer()
                    if !store.visibleIDs.isEmpty {
                        Button("\(store.visibleIDs.count)장 모두 픽") {
                            store.flag(ids: store.visibleIDs, flag: 1)
                        }
                        .font(.caption.weight(.semibold))
                        .buttonStyle(.borderedProminent)
                        .tint(palette.success)
                    }
                }
                HStack(spacing: 8) {
                    Text("상위 \(store.keepPct)%")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(palette.text2)
                        .frame(width: 64, alignment: .leading)
                    Slider(
                        value: Binding(
                            get: { Double(store.keepPct) },
                            set: { store.updateKeepPct(Int($0.rounded())) }
                        ),
                        in: 5...50,
                        step: 5
                    )
                    .tint(palette.success)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(palette.toolbar)
            .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }

        case .rejected where !store.visibleIDs.isEmpty:
            HStack {
                Text("거절한 사진 \(store.visibleIDs.count)장")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(palette.text)
                Spacer()
                Button("\(store.visibleIDs.count)장 휴지통으로…") {
                    store.askDelete(ids: store.visibleIDs)
                }
                .font(.caption.weight(.semibold))
                .buttonStyle(.borderedProminent)
                .tint(palette.danger)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(palette.toolbar)
            .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }

        default:
            EmptyView()
        }
    }

    @ViewBuilder
    private var bottomBar: some View {
        if store.isSelectionMode {
            VStack(spacing: 0) {
                Divider().overlay(palette.separator)
                HStack(spacing: 10) {
                    Button(store.selectedIDs.count == store.visibleIDs.count ? "선택 해제" : "모두 선택") {
                        if store.selectedIDs.count == store.visibleIDs.count {
                            store.clearSelection()
                        } else {
                            store.selectAllVisible()
                        }
                    }
                    .font(.caption.weight(.medium))

                    Spacer()

                    let ids = Array(store.selectedIDs)
                    Button { store.flag(ids: ids, flag: 1) } label: { Image(systemName: "flag.fill") }
                        .tint(palette.success)
                        .disabled(ids.isEmpty)
                    Button { store.flag(ids: ids, flag: -1) } label: { Image(systemName: "xmark.circle.fill") }
                        .tint(palette.danger)
                        .disabled(ids.isEmpty)
                    Button { store.rate(ids: ids, rating: 3) } label: { Text("★★★").font(.caption2.weight(.bold)) }
                        .tint(palette.star)
                        .disabled(ids.isEmpty)
                    Button { store.openExport(for: ids) } label: { Image(systemName: "square.and.arrow.up") }
                        .tint(palette.accent)
                        .disabled(ids.isEmpty)
                    Button(role: .destructive) { store.askDelete(ids: ids) } label: { Image(systemName: "trash") }
                        .disabled(ids.isEmpty)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(palette.toolbar)
            }
        } else if let toast = store.toast {
            VStack(spacing: 0) {
                Divider().overlay(palette.separator)
                HStack {
                    Spacer()
                    Text(toast.text)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background(toastColor(toast.tone), in: Capsule())
                    Spacer()
                }
                .padding(.vertical, 6)
                .background(palette.toolbar)
            }
        }
    }

    private var loadingPlaceholder: some View {
        VStack(spacing: 12) {
            ProgressView().tint(palette.accent)
            Text("라이브러리를 여는 중…")
                .font(.subheadline)
                .foregroundStyle(palette.text2)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var emptyState: some View {
        VStack(spacing: 14) {
            Image("Mascot")
                .resizable()
                .scaledToFit()
                .frame(width: 112, height: 112)
            Text(store.counts.all == 0 ? "SD 카드나 외장 SSD를 연결해 보세요" : "이 보기에 해당하는 사진이 없습니다")
                .font(.headline)
                .foregroundStyle(palette.text)
                .multilineTextAlignment(.center)
            Text(
                store.counts.all == 0
                    ? "USB-C 카드 리더로 사진을 가져오면 EXIF 날짜별로 정리되고, 셀렉 도우미가 기기 안에서 B컷과 A컷을 찾아 줍니다."
                    : "다른 필터를 선택하거나 새 사진을 가져올 수 있습니다."
            )
            .font(.subheadline)
            .foregroundStyle(palette.text2)
            .multilineTextAlignment(.center)
            .frame(maxWidth: 360)

            HStack(spacing: 10) {
                Button("SD 카드 · 폴더 가져오기") {
                    store.openImport()
                }
                .buttonStyle(.borderedProminent)
                .tint(palette.accent)

                if store.counts.all == 0 {
                    Button("외장 SSD 라이브러리 열기") {
                        store.settingsSheetPresented = true
                    }
                    .buttonStyle(.bordered)
                    .tint(palette.text2)
                }
            }
            .padding(.top, 4)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
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
