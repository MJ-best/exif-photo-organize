import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Sheet for scanning and checksum-verified importing from an SD card, USB-C drive, or Files folder.
struct ImportSheetView: View {
    @Bindable var store: LibraryStore
    @State private var folderPickerPresented = false
    @Environment(\.loupePalette) private var palette
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 18) {
                switch store.importPhase {
                case .idle:
                    idleContent
                case .scanning(let label):
                    VStack(spacing: 12) {
                        ProgressView().controlSize(.large).tint(palette.accent)
                        Text("'\(label)' 카드 살펴보는 중…")
                            .font(.headline)
                            .foregroundStyle(palette.text)
                        Text("EXIF 촬영 일시와 이미 가져온 사진을 확인하고 있습니다.")
                            .font(.caption)
                            .foregroundStyle(palette.text2)
                    }
                    .frame(maxHeight: .infinity)

                case let .ready(info, includeVideos, freeBytes):
                    readyContent(info: info, includeVideos: includeVideos, freeBytes: freeBytes)

                case let .copying(status, label):
                    copyingContent(status: status, label: label)

                case .done(let result):
                    doneContent(result)

                case .failed(let error):
                    VStack(spacing: 12) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .font(.largeTitle)
                            .foregroundStyle(palette.danger)
                        Text(error.title)
                            .font(.headline)
                            .foregroundStyle(palette.text)
                        Text(error.message)
                            .font(.subheadline)
                            .foregroundStyle(palette.text2)
                            .multilineTextAlignment(.center)
                        Button("다른 폴더 고르기") {
                            folderPickerPresented = true
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(palette.accent)
                    }
                    .padding()
                    .frame(maxHeight: .infinity)
                }
            }
            .padding(20)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.content)
            .navigationTitle("사진 가져오기")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    if case .copying = store.importPhase {
                        EmptyView()
                    } else {
                        Button("닫기") { dismiss() }
                    }
                }
            }
            .sheet(isPresented: $folderPickerPresented) {
                FolderDocumentPicker { url in
                    store.scanCardFolder(url: url, includeVideos: true)
                }
            }
        }
    }

    private var idleContent: some View {
        VStack(spacing: 14) {
            Image(systemName: "sdcard.fill")
                .font(.system(size: 48))
                .foregroundStyle(palette.accent)
            Text("SD 카드 또는 사진 폴더 선택")
                .font(.headline)
                .foregroundStyle(palette.text)
            Text("USB-C 카드 리더를 꽂고 파일 앱에서 카드(DCIM)나 폴더를 고르면 날짜별로 복사·검증하고 RAW+JPEG를 한 장으로 묶습니다. 이미 가져온 사진은 자동으로 건너뜁니다.")
                .font(.subheadline)
                .foregroundStyle(palette.text2)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            Button("카드 · 폴더 선택하기") {
                folderPickerPresented = true
            }
            .buttonStyle(.borderedProminent)
            .tint(palette.accent)
            .padding(.top, 6)
        }
        .frame(maxHeight: .infinity)
    }

    private func readyContent(info: LoupeScanInfo, includeVideos: Bool, freeBytes: Int64) -> some View {
        let lowSpace = freeBytes >= 0 && info.newBytes > freeBytes
        return VStack(alignment: .leading, spacing: 14) {
            HStack {
                Label(info.label, systemImage: "sdcard")
                    .font(.headline)
                    .foregroundStyle(palette.text)
                Spacer()
                Button("다시 고르기") { folderPickerPresented = true }
                    .font(.caption)
            }

            VStack(alignment: .leading, spacing: 8) {
                summaryRow("새로 가져올 사진", "\(LoupeFormat.count(info.newPhotos))장 (전체 \(LoupeFormat.count(info.photos))장)")
                summaryRow("새 파일 용량", LoupeFormat.bytes(info.newBytes))
                if freeBytes >= 0 {
                    summaryRow("라이브러리 여유 공간", LoupeFormat.bytes(freeBytes))
                }
                if info.rawPairs > 0 {
                    summaryRow("RAW + JPEG 묶음", "\(LoupeFormat.count(info.rawPairs))쌍")
                }
                if !info.cameras.isEmpty {
                    summaryRow("카메라", info.cameras.joined(separator: ", "))
                }
            }
            .padding(14)
            .background(palette.panel, in: RoundedRectangle(cornerRadius: 12))

            Toggle("동영상 함께 가져오기", isOn: Binding(
                get: { includeVideos },
                set: { store.setImportIncludeVideos($0) }
            ))
            .tint(palette.accent)

            if lowSpace {
                Label("남은 저장 공간이 부족합니다. 휴지통을 비우거나 외장 SSD를 라이브러리로 지정해 주세요.", systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(palette.danger)
            }

            Spacer()

            Button {
                store.startImport()
            } label: {
                Text(info.newPhotos > 0 ? "새 사진 \(LoupeFormat.count(info.newPhotos))장 가져오기" : "모두 이미 가져온 사진입니다")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .tint(palette.accent)
            .disabled(info.newPhotos == 0 || lowSpace)
        }
    }

    private func copyingContent(status: LoupeImportStatus, label: String) -> some View {
        VStack(spacing: 14) {
            Text("'\(label)'에서 가져오는 중…")
                .font(.headline)
                .foregroundStyle(palette.text)
            ProgressView(value: status.fraction)
                .tint(palette.accent)
            HStack {
                Text("\(status.filesDone) / \(status.filesTotal) 파일")
                Spacer()
                Text("\(LoupeFormat.bytes(status.bytesDone)) / \(LoupeFormat.bytes(status.bytesTotal))")
            }
            .font(.caption.monospacedDigit())
            .foregroundStyle(palette.text2)
            if !status.current.isEmpty {
                Text(status.current)
                    .font(.caption2)
                    .foregroundStyle(palette.text3)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Button("중지", role: .destructive) {
                store.cancelImport()
            }
            .buttonStyle(.bordered)
            .padding(.top, 8)
        }
        .frame(maxHeight: .infinity)
    }

    private func doneContent(_ result: LoupeImportResult) -> some View {
        VStack(spacing: 12) {
            Image(systemName: result.cancelled ? "stop.circle.fill" : "checkmark.circle.fill")
                .font(.system(size: 46))
                .foregroundStyle(result.cancelled ? palette.warning : palette.success)
            Text(result.cancelled ? "가져오기를 중지했습니다" : "가져오기를 마쳤습니다")
                .font(.headline)
                .foregroundStyle(palette.text)
            Text("새 사진 \(result.added)장 · 중복 건너뜀 \(result.duplicates)장 · \(LoupeFormat.bytes(result.bytes))")
                .font(.subheadline)
                .foregroundStyle(palette.text2)
            if !result.failures.isEmpty {
                Text("실패 \(result.failures.count)건: \(result.failures.first ?? "")")
                    .font(.caption)
                    .foregroundStyle(palette.danger)
            }
            Button("라이브러리에서 보기") {
                dismiss()
            }
            .buttonStyle(.borderedProminent)
            .tint(palette.accent)
            .padding(.top, 8)
        }
        .frame(maxHeight: .infinity)
    }

    private func summaryRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).font(.caption).foregroundStyle(palette.text2)
            Spacer()
            Text(value).font(.caption.weight(.semibold)).foregroundStyle(palette.text).multilineTextAlignment(.trailing)
        }
    }
}

/// SwiftUI bridge around `UIDocumentPickerViewController` for selecting SD cards / folders.
struct FolderDocumentPicker: UIViewControllerRepresentable {
    let onPick: (URL) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onPick: onPick) }

    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.folder], asCopy: false)
        picker.allowsMultipleSelection = false
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: UIDocumentPickerViewController, context: Context) {}

    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let onPick: (URL) -> Void
        init(onPick: @escaping (URL) -> Void) { self.onPick = onPick }

        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            if let first = urls.first {
                onPick(first)
            }
        }
    }
}
