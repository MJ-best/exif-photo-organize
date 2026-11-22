import SwiftUI

/// Adaptive root view:
/// - iPad / Regular width: `NavigationSplitView` (sidebar + grid)
/// - iPhone / Compact width: `NavigationStack` with top filter ribbon
/// Also binds full-screen Viewer, Import/Export/Settings sheets, delete confirmation, and
/// structured error recovery alerts.
struct RootView: View {
    @Bindable var store: LibraryStore
    @Environment(\.horizontalSizeClass) private var sizeClass
    @Environment(\.loupePalette) private var palette

    var body: some View {
        ZStack {
            if sizeClass == .regular {
                NavigationSplitView {
                    SidebarView(store: store)
                } detail: {
                    NavigationStack {
                        LibraryGridView(store: store)
                    }
                }
                .navigationSplitViewStyle(.balanced)
            } else {
                NavigationStack {
                    LibraryGridView(store: store)
                }
            }

            if let session = store.viewer {
                ViewerView(store: store, session: session)
                    .transition(.opacity)
                    .zIndex(10)
            }
        }
        .tint(palette.accent)
        .sheet(isPresented: $store.importSheetPresented) {
            ImportSheetView(store: store)
        }
        .sheet(isPresented: $store.exportSheetPresented) {
            ExportSheetView(store: store)
        }
        .sheet(isPresented: $store.settingsSheetPresented) {
            SettingsSheetView(store: store)
        }
        .confirmationDialog(
            "휴지통으로 옮길까요?",
            isPresented: Binding(
                get: { store.deleteConfirmation != nil },
                set: { if !$0 { store.deleteConfirmation = nil } }
            ),
            titleVisibility: .visible,
            presenting: store.deleteConfirmation
        ) { confirmation in
            Button("\(confirmation.plan.photos)장 휴지통으로 옮기기", role: .destructive) {
                store.confirmDelete()
            }
            Button("취소", role: .cancel) {
                store.deleteConfirmation = nil
            }
        } message: { confirmation in
            Text(
                "사진 \(confirmation.plan.photos)장 (파일 \(confirmation.plan.files)개 · \(LoupeFormat.bytes(confirmation.plan.bytes)))을 라이브러리 휴지통으로 옮깁니다. RAW·JPEG·XMP가 함께 이동하며 셀렉 학습 기록은 남습니다."
            )
        }
        .alert(
            store.recoveryAlert?.error.title ?? "",
            isPresented: Binding(
                get: { store.recoveryAlert != nil },
                set: { if !$0 { store.recoveryAlert = nil } }
            ),
            presenting: store.recoveryAlert
        ) { alert in
            if let title = alert.primaryActionTitle, let action = alert.primaryAction {
                Button(title) {
                    store.performRecoveryAction(action)
                }
            }
            Button("닫기", role: .cancel) {
                store.recoveryAlert = nil
            }
        } message: { alert in
            Text(alert.error.message)
        }
    }
}

#Preview("LOUPE · 크림") {
    let mock = MockPhotoEngine(sampleCount: 16)
    let store = LibraryStore(engine: mock, engineFactory: { _ in mock })
    return RootView(store: store)
        .environment(\.loupePalette, .cream)
        .task { await store.bootstrap() }
}

#Preview("LOUPE · 코코아") {
    let mock = MockPhotoEngine(sampleCount: 16)
    let store = LibraryStore(engine: mock, engineFactory: { _ in mock })
    return RootView(store: store)
        .environment(\.loupePalette, .cocoa)
        .preferredColorScheme(.dark)
        .task { await store.bootstrap() }
}
