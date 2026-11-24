import SwiftUI

/// iPad sidebar for `NavigationSplitView`, matching LOUPE desktop organization.
struct SidebarView: View {
    @Bindable var store: LibraryStore
    @Environment(\.loupePalette) private var palette

    var body: some View {
        List {
            Section("라이브러리") {
                sidebarRow(.all)
                sidebarRow(.undecided)
            }

            Section("셀렉 도우미") {
                sidebarRow(.candidates)
                sidebarRow(.keepers)
            }

            Section("분류") {
                sidebarRow(.picked)
                sidebarRow(.rejected)
                sidebarRow(.rated)
            }

            Section("작업") {
                Button {
                    store.openImport()
                } label: {
                    Label("SD 카드 · 폴더 가져오기", systemImage: "sdcard")
                        .foregroundStyle(palette.accent)
                }
                Button {
                    store.guideSheetPresented = true
                } label: {
                    Label("사용 가이드 · 조작법", systemImage: "questionmark.circle")
                        .foregroundStyle(palette.text)
                }
                Button {
                    store.refreshTrashBytes()
                    store.settingsSheetPresented = true
                } label: {
                    Label("설정 · 라이브러리 위치", systemImage: "gearshape")
                        .foregroundStyle(palette.text)
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(palette.sidebar)
        .navigationTitle("LOUPE")
        .safeAreaInset(edge: .bottom) {
            VStack(alignment: .leading, spacing: 4) {
                Divider().overlay(palette.separator)
                HStack(spacing: 6) {
                    Image(systemName: store.isSampleMode ? "sparkles" : (store.isExternalLibrary ? "externaldrive.fill" : "ipad"))
                        .font(.caption)
                        .foregroundStyle(palette.accent)
                    Text(store.libraryDisplayName)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(palette.text)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Text(store.learn.summary)
                    .font(.caption2)
                    .foregroundStyle(palette.text3)
                    .lineLimit(2)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(palette.sidebar)
        }
    }

    private func sidebarRow(_ filter: LoupeViewFilter) -> some View {
        let active = store.activeView == filter
        let count = store.counts.count(for: filter)
        return Button {
            store.selectView(filter)
        } label: {
            HStack {
                Label(filter.title, systemImage: filter.systemImage)
                    .foregroundStyle(active ? palette.accent : palette.text)
                    .lineLimit(1)
                Spacer()
                if count > 0 {
                    Text(LoupeFormat.count(count))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(active ? palette.accent : palette.text3)
                }
            }
            .padding(.vertical, 2)
        }
        .listRowBackground(active ? palette.selected : Color.clear)
    }
}
