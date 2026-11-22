import SwiftUI

/// Settings sheet covering library location (on-device vs external USB-C SSD), all 17 B-cut rule
/// toggles, culling assistant status, and library trash cleanup.
struct SettingsSheetView: View {
    @Bindable var store: LibraryStore
    @State private var libraryPickerPresented = false
    @State private var confirmEmptyTrash = false
    @Environment(\.loupePalette) private var palette
    @Environment(\.dismiss) private var dismiss

    private var groupedRules: [(group: String, items: [LoupeRuleItem])] {
        var order: [String] = []
        var map: [String: [LoupeRuleItem]] = [:]
        for rule in store.rules {
            if map[rule.group] == nil { order.append(rule.group) }
            map[rule.group, default: []].append(rule)
        }
        return order.map { ($0, map[$0] ?? []) }
    }

    private var thermalBudgetSummary: String {
        let monitor = store.analyzer.thermalMonitor
        let threads = monitor.recommendedThreads
        let stateLabel: String
        switch monitor.thermalState {
        case .nominal: stateLabel = "정상"
        case .fair: stateLabel = "약간 따뜻함"
        case .serious: stateLabel = "발열 주의"
        case .critical: stateLabel = "과열 일시정지"
        @unknown default: stateLabel = "자동"
        }
        if threads == 0 {
            return "일시정지 (\(stateLabel))"
        }
        let lowPower = monitor.lowPowerMode ? " · 저전력" : ""
        return "최대 \(threads)스레드 (\(stateLabel)\(lowPower))"
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    LabeledContent("현재 위치", value: store.libraryDisplayName)
                    Button("외장 SSD · 파일 앱 폴더로 변경…") {
                        libraryPickerPresented = true
                    }
                    .foregroundStyle(palette.accent)

                    if store.isExternalLibrary {
                        Button("기기 내장 라이브러리로 되돌리기") {
                            Task { await store.switchToDefaultLibrary() }
                        }
                    }
                } header: {
                    Text("라이브러리 위치")
                } footer: {
                    Text(
                        store.isExternalLibrary
                            ? "외장 SSD의 .photo-organize 카탈로그를 맥·윈도우 LOUPE와 그대로 이어서 씁니다."
                            : "대용량 RAW 사진이 iCloud 백업을 채우지 않도록 자동 제외됩니다. 단, 앱을 지우면 기기 내장 라이브러리의 사진도 함께 지워집니다."
                    )
                }

                Section("셀렉 도우미") {
                    LabeledContent("학습 상태", value: store.learn.summary)
                    LabeledContent("분석 완료", value: "\(store.learn.analysed) / \(store.learn.total)장")
                    LabeledContent("분석 스레드 예산", value: thermalBudgetSummary)
                    Button("미분석 사진 지금 분석하기") {
                        store.startAnalysisIfNeeded()
                    }
                    .foregroundStyle(palette.accent)
                }

                ForEach(groupedRules, id: \.group) { section in
                    Section("B컷으로 볼 기준 · \(section.group)") {
                        ForEach(section.items) { rule in
                            Toggle(rule.label, isOn: Binding(
                                get: { rule.enabled },
                                set: { _ in store.toggleRule(bit: rule.bit) }
                            ))
                            .tint(palette.accent)
                        }
                    }
                }

                Section {
                    LabeledContent("휴지통 용량", value: LoupeFormat.bytes(store.trashBytes))
                    Button("휴지통 비우기", role: .destructive) {
                        confirmEmptyTrash = true
                    }
                    .disabled(store.trashBytes == 0)

                    Button {
                        store.optimizeStorage()
                    } label: {
                        HStack {
                            Text("카탈로그 최적화 (WAL 정리)")
                            Spacer()
                            if store.isOptimizingStorage {
                                ProgressView().controlSize(.small)
                            }
                        }
                    }
                    .foregroundStyle(palette.accent)
                    .disabled(store.isOptimizingStorage)
                } header: {
                    Text("저장소 · 휴지통")
                } footer: {
                    Text("거절 후 휴지통으로 옮긴 사진은 비우기 전까지 보관되며, 비워도 셀렉 도우미 학습 기록은 남습니다. 외장 SSD를 분리하기 전에 WAL 정리를 실행하면 다른 기기에서도 안전하게 바로 열 수 있습니다.")
                }

                Section("개인정보 · 보안") {
                    Label("모든 사진과 AI 모델은 기기 안에서만 동작합니다 (네트워크 전송 0)", systemImage: "lock.shield")
                        .font(.caption)
                        .foregroundStyle(palette.text2)
                }
            }
            .scrollContentBackground(.hidden)
            .background(palette.content)
            .navigationTitle("설정")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("완료") { dismiss() }
                }
            }
            .sheet(isPresented: $libraryPickerPresented) {
                FolderDocumentPicker { url in
                    Task { await store.switchToExternalLibrary(url: url) }
                }
            }
            .confirmationDialog(
                "휴지통의 파일을 영구 삭제할까요?",
                isPresented: $confirmEmptyTrash,
                titleVisibility: .visible
            ) {
                Button("\(LoupeFormat.bytes(store.trashBytes)) 영구 삭제", role: .destructive) {
                    store.emptyTrash()
                }
                Button("취소", role: .cancel) {}
            } message: {
                Text("삭제된 파일은 되돌릴 수 없지만, 셀렉 도우미의 학습 기록은 유지됩니다.")
            }
        }
    }
}
