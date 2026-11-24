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
            if sizeClass == .compact && store.counts.all > 0 {
                compactFilterRibbon
            }

            if store.isSampleMode {
                sampleModeBanner
            }

            if let progress = store.analyzer.progress {
                analysisBanner(progress)
            } else if shouldShowAnalysisNextStep {
                analysisNextStepBanner
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
        .navigationTitle(store.counts.all == 0 ? "LOUPE" : store.activeView.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .topBarTrailing) {
                if store.canUndo && !store.isSelectionMode {
                    Button {
                        store.undoLastDecision()
                    } label: {
                        Image(systemName: "arrow.uturn.backward")
                    }
                    .accessibilityLabel("실행 취소")
                }

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

                Button {
                    store.guideSheetPresented = true
                } label: {
                    Image(systemName: "questionmark.circle")
                }
                .accessibilityLabel("사용 가이드")

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

    private var shouldShowAnalysisNextStep: Bool {
        store.activeView == .all
            && !store.hasDismissedAnalysisNextStep
            && store.counts.analysed > 0
            && (store.counts.candidates > 0 || store.counts.keepers > 0)
    }

    private var sampleModeBanner: some View {
        HStack(spacing: 10) {
            Image(systemName: "sparkles")
                .foregroundStyle(palette.accent)
            Text("견본 사진 체험 중")
                .font(.caption.weight(.semibold))
                .foregroundStyle(palette.text)
                .lineLimit(1)
            Spacer()
            Button("체험 끝내기") {
                Task { await store.exitSampleMode() }
            }
            .font(.caption.weight(.semibold))
            .buttonStyle(.bordered)
            .controlSize(.mini)
            .tint(palette.accent)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(palette.selected.opacity(0.65))
        .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }
    }

    private var analysisNextStepBanner: some View {
        let hasCandidates = store.counts.candidates > 0
        let summaryText: String = {
            if store.counts.candidates > 0 && store.counts.keepers > 0 {
                return "분석 완료 · B컷 후보 \(store.counts.candidates)장 · A컷 후보 \(store.counts.keepers)장"
            } else if store.counts.candidates > 0 {
                return "분석 완료 · B컷 후보 \(store.counts.candidates)장 찾음"
            } else {
                return "분석 완료 · A컷 후보 \(store.counts.keepers)장 찾음"
            }
        }()

        return HStack(spacing: 8) {
            Image(systemName: "wand.and.stars")
                .font(.caption)
                .foregroundStyle(palette.accent)
            Text(summaryText)
                .font(.caption.weight(.semibold))
                .foregroundStyle(palette.text)
                .lineLimit(1)
            Spacer(minLength: 4)
            Button(hasCandidates ? "B컷부터 보기" : "A컷부터 보기") {
                store.dismissAnalysisNextStep()
                store.selectView(hasCandidates ? .candidates : .keepers)
            }
            .font(.caption.weight(.semibold))
            .buttonStyle(.borderedProminent)
            .controlSize(.mini)
            .tint(palette.accent)

            Button {
                store.dismissAnalysisNextStep()
            } label: {
                Image(systemName: "xmark")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(palette.text3)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("안내 닫기")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
        .background(palette.selected.opacity(0.45))
        .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }
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

        case .picked where !store.visibleIDs.isEmpty:
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("픽한 A컷 \(store.visibleIDs.count)장")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(palette.text)
                    Text("사진 앱 · AirDrop · 파일 · Lightroom 전송")
                        .font(.caption2)
                        .foregroundStyle(palette.text2)
                        .lineLimit(1)
                }
                Spacer()
                Button("\(store.visibleIDs.count)장 내보내기…") {
                    store.openExport(for: store.visibleIDs)
                }
                .font(.caption.weight(.semibold))
                .buttonStyle(.borderedProminent)
                .tint(palette.accent)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(palette.toolbar)
            .overlay(alignment: .bottom) { Divider().overlay(palette.separator) }

        case .rated where !store.visibleIDs.isEmpty:
            HStack {
                Text("별점 준 사진 \(store.visibleIDs.count)장")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(palette.text)
                Spacer()
                Button("\(store.visibleIDs.count)장 내보내기…") {
                    store.openExport(for: store.visibleIDs)
                }
                .font(.caption.weight(.semibold))
                .buttonStyle(.borderedProminent)
                .tint(palette.accent)
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
                HStack(spacing: 10) {
                    Spacer()
                    Text(toast.text)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background(toastColor(toast.tone), in: Capsule())
                    if toast.undoAvailable && store.canUndo {
                        Button("되돌리기") {
                            store.undoLastDecision()
                        }
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(palette.accent)
                        .buttonStyle(.plain)
                    }
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
        ScrollView {
            if store.counts.all == 0 {
                emptyLibraryWelcome
            } else {
                emptyFilteredView
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var emptyLibraryWelcome: some View {
        VStack(spacing: 16) {
            // Current LOUPE library space pill
            Button {
                store.refreshTrashBytes()
                store.settingsSheetPresented = true
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: store.isExternalLibrary ? "externaldrive.fill" : "iphone")
                        .font(.caption)
                        .foregroundStyle(palette.accent)
                    Text("저장 공간:")
                        .font(.caption)
                        .foregroundStyle(palette.text2)
                    Text(store.libraryDisplayName)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(palette.text)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Text("변경")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(palette.accent)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 7)
                .background(palette.panel, in: Capsule())
                .overlay(Capsule().stroke(palette.separator, lineWidth: 1))
            }
            .buttonStyle(.plain)
            .padding(.top, 8)

            Image("Mascot")
                .resizable()
                .scaledToFit()
                .frame(width: 88, height: 88)

            VStack(spacing: 6) {
                Text("첫 사진을 가져와 보세요")
                    .font(.title3.weight(.bold))
                    .foregroundStyle(palette.text)
                    .multilineTextAlignment(.center)
                Text("SD 카드에서 LOUPE 공간으로 복사하면 날짜 정리와 AI B컷 · A컷 분석이 자동으로 시작됩니다.")
                    .font(.subheadline)
                    .foregroundStyle(palette.text2)
                    .multilineTextAlignment(.center)
            }

            // 3-step visual mini guide
            VStack(alignment: .leading, spacing: 10) {
                emptyStepRow("1", "USB-C 카드 리더로 SD 카드 연결")
                emptyStepRow("2", "파일 앱에서 카드 → DCIM 폴더 선택")
                emptyStepRow("3", "AI가 찾은 B컷 후보 거르고 A컷 내보내기")
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(palette.panel, in: RoundedRectangle(cornerRadius: 16))
            .overlay(RoundedRectangle(cornerRadius: 16).stroke(palette.separator, lineWidth: 1))

            // Full-width stacked buttons (never wrap unevenly on compact iPhones)
            VStack(spacing: 10) {
                Button {
                    store.openImport()
                } label: {
                    Text("SD 카드 · 폴더에서 가져오기")
                        .font(.body.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 4)
                }
                .buttonStyle(.borderedProminent)
                .tint(palette.accent)
                .controlSize(.large)

                Button {
                    store.externalLibraryPickerPresented = true
                } label: {
                    Text("외장 SSD 라이브러리 바로 열기")
                        .font(.subheadline.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 3)
                }
                .buttonStyle(.bordered)
                .tint(palette.text)
                .controlSize(.large)

                Button {
                    Task { await store.enterSampleMode() }
                } label: {
                    if store.isLoadingSampleMode {
                        HStack(spacing: 8) {
                            ProgressView().controlSize(.small)
                            Text("견본 사진 불러오는 중…")
                        }
                        .font(.subheadline.weight(.semibold))
                    } else {
                        Text("SD 카드가 없나요? 견본 사진 5장으로 체험해 보기")
                            .font(.subheadline.weight(.semibold))
                    }
                }
                .foregroundStyle(palette.accent)
                .disabled(store.isLoadingSampleMode)
                .padding(.top, 4)
            }
        }
        .frame(maxWidth: 420)
        .padding(.horizontal, 20)
        .padding(.vertical, 16)
        .frame(maxWidth: .infinity)
    }

    private func emptyStepRow(_ num: String, _ text: String) -> some View {
        HStack(spacing: 10) {
            Text(num)
                .font(.caption2.weight(.bold))
                .foregroundStyle(palette.accent)
                .frame(width: 22, height: 22)
                .background(palette.selected, in: Circle())
            Text(text)
                .font(.caption.weight(.medium))
                .foregroundStyle(palette.text)
            Spacer(minLength: 0)
        }
    }

    private var emptyFilteredView: some View {
        VStack(spacing: 14) {
            Image("Mascot")
                .resizable()
                .scaledToFit()
                .frame(width: 84, height: 84)
            Text("'\(store.activeView.title)'에 해당하는 사진이 없습니다")
                .font(.headline)
                .foregroundStyle(palette.text)
                .multilineTextAlignment(.center)
            Text(emptyFilterHint)
                .font(.subheadline)
                .foregroundStyle(palette.text2)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 340)

            Button("전체 사진으로 돌아가기") {
                store.selectView(.all)
            }
            .buttonStyle(.borderedProminent)
            .tint(palette.accent)
            .padding(.top, 4)
        }
        .padding(32)
        .frame(maxWidth: .infinity)
    }

    private var emptyFilterHint: String {
        switch store.activeView {
        case .candidates:
            return "현재 B컷 기준(\(store.threshold)%)에 해당하는 사진이 없습니다. 위 슬라이더를 낮추면 더 엄격하게 찾아볼 수 있습니다."
        case .keepers:
            return "분석이 끝나면 선명도와 구도가 좋은 상위 사진이 여기에 자동으로 모입니다."
        case .picked:
            return "사진을 열어 위로 밀거나 깃발 버튼을 누르면 남길 A컷으로 모입니다."
        case .rejected:
            return "사진을 열어 아래로 밀거나 X 버튼으로 거절한 사진이 여기에 모입니다."
        case .rated:
            return "사진 보기에서 별점(★)을 준 사진이 여기에 모입니다."
        case .undecided:
            return "모든 사진의 분류를 마쳤습니다."
        case .all:
            return "새 사진을 가져올 수 있습니다."
        }
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
