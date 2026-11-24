import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Sheet for scanning and checksum-verified importing from an SD card, USB-C drive, or Files folder.
struct ImportSheetView: View {
    @Bindable var importer: ImportCoordinator
    var destinationName: String = "이 기기 · LOUPE"
    var isExternalLibrary: Bool = false
    var freeCapacityBytes: Int64 = -1
    var onChangeDestination: (() -> Void)? = nil

    @State private var folderPickerPresented = false
    @Environment(\.loupePalette) private var palette
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 18) {
                switch importer.phase {
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
                    if case .copying = importer.phase {
                        EmptyView()
                    } else {
                        Button("닫기") { dismiss() }
                    }
                }
            }
            .sheet(isPresented: $folderPickerPresented) {
                FolderDocumentPicker { url in
                    importer.scanCardFolder(url: url, includeVideos: true)
                }
            }
        }
    }

    private var idleContent: some View {
        ScrollView {
            VStack(spacing: 16) {
                // Source -> Destination summary card
                VStack(spacing: 12) {
                    HStack(spacing: 12) {
                        Image(systemName: "sdcard.fill")
                            .font(.title3)
                            .foregroundStyle(palette.accent)
                            .frame(width: 28)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("어디서 가져오나요?")
                                .font(.caption2)
                                .foregroundStyle(palette.text3)
                            Text("SD 카드 또는 사진 폴더")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(palette.text)
                        }
                        Spacer()
                        Image(systemName: "arrow.down")
                            .font(.caption.weight(.bold))
                            .foregroundStyle(palette.accent)
                    }

                    Divider().overlay(palette.separator)

                    HStack(spacing: 12) {
                        Image(systemName: isExternalLibrary ? "externaldrive.fill" : "iphone")
                            .font(.title3)
                            .foregroundStyle(palette.accent)
                            .frame(width: 28)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("어디에 저장되나요? (LOUPE 앱 공간)")
                                .font(.caption2)
                                .foregroundStyle(palette.text3)
                            Text(destinationSummaryText)
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(palette.text)
                                .lineLimit(2)
                        }
                        Spacer()
                        if let onChangeDestination {
                            Button("변경") {
                                onChangeDestination()
                            }
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(palette.accent)
                        }
                    }
                }
                .padding(15)
                .background(palette.panel, in: RoundedRectangle(cornerRadius: 16))
                .overlay(RoundedRectangle(cornerRadius: 16).stroke(palette.separator, lineWidth: 1))

                // Visual DCIM folder selection guide
                VStack(alignment: .leading, spacing: 10) {
                    Text("파일 앱이 열리면 이렇게 고르세요")
                        .font(.subheadline.weight(.bold))
                        .foregroundStyle(palette.text)

                    VStack(alignment: .leading, spacing: 8) {
                        importGuideStep("1", "왼쪽(또는 둘러보기)에서 SD 카드 이름을 선택합니다.")
                        importGuideStep("2", "DCIM 폴더를 누른 뒤 우측 상단 「열기」를 탭합니다.")
                    }
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(palette.toolbar, in: RoundedRectangle(cornerRadius: 12))

                    Text("이미 가져온 사진은 자동으로 건너뛰며, 같은 컷의 RAW+JPEG는 한 장으로 묶입니다.")
                        .font(.caption)
                        .foregroundStyle(palette.text3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(15)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(palette.content, in: RoundedRectangle(cornerRadius: 16))
                .overlay(RoundedRectangle(cornerRadius: 16).stroke(palette.separator, lineWidth: 1))

                Button {
                    folderPickerPresented = true
                } label: {
                    Text("파일 앱에서 SD 카드(DCIM) 고르기")
                        .font(.body.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 4)
                }
                .buttonStyle(.borderedProminent)
                .tint(palette.accent)
                .controlSize(.large)
                .padding(.top, 4)
            }
            .frame(maxWidth: 460)
            .frame(maxWidth: .infinity)
        }
    }

    private var destinationSummaryText: String {
        if freeCapacityBytes >= 0 {
            return "\(destinationName) (여유 \(LoupeFormat.bytes(freeCapacityBytes)))"
        }
        return destinationName
    }

    private func importGuideStep(_ number: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text(number)
                .font(.caption2.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 18, height: 18)
                .background(palette.accent, in: Circle())
            Text(text)
                .font(.caption)
                .foregroundStyle(palette.text2)
                .fixedSize(horizontal: false, vertical: true)
        }
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
                set: { importer.setIncludeVideos($0) }
            ))
            .tint(palette.accent)

            if lowSpace {
                Label("남은 저장 공간이 부족합니다. 휴지통을 비우거나 외장 SSD를 라이브러리로 지정해 주세요.", systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(palette.danger)
            }

            Spacer()

            Button {
                importer.startImport()
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
                importer.cancel()
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
            if !result.cancelled && result.added > 0 {
                Text("AI 셀렉 도우미가 기기 안에서 B컷과 A컷을 자동으로 분석합니다.")
                    .font(.caption)
                    .foregroundStyle(palette.text3)
                    .multilineTextAlignment(.center)
            }
            Button(result.added > 0 ? "지금 바로 셀렉 시작하기" : "라이브러리에서 보기") {
                dismiss()
            }
            .buttonStyle(.borderedProminent)
            .tint(palette.accent)
            .controlSize(.large)
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
