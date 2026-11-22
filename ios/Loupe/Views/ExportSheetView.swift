import SwiftUI
import UIKit

/// Export selected photos as JPEG-only, resized (GPS stripped), or originals (RAW + XMP) and hand
/// them off via the system Share Sheet (AirDrop, Files, Lightroom Mobile).
struct ExportSheetView: View {
    @Bindable var store: LibraryStore
    @State private var mode: LoupeExportMode = .jpegOnly
    @State private var longEdge: Int = 2048
    @State private var quality: Int = 88
    @State private var dateNames: Bool = true
    @State private var shareItems: [URL] = []
    @State private var sharePresented: Bool = false

    @Environment(\.loupePalette) private var palette
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                switch store.exportPhase {
                case .idle, .planning:
                    ProgressView("내보내기 준비 중…")
                        .tint(palette.accent)
                        .frame(maxHeight: .infinity)

                case let .ready(_, plan):
                    Form {
                        Section("형식") {
                            Picker("내보내기 형식", selection: $mode) {
                                ForEach(LoupeExportMode.allCases) { item in
                                    Text(item.title).tag(item)
                                }
                            }
                            .pickerStyle(.inline)
                        }

                        if mode == .resized {
                            Section("작게 내보내기 설정 (GPS 제거)") {
                                Picker("긴 변 크기", selection: $longEdge) {
                                    Text("1080 px (SNS)").tag(1080)
                                    Text("2048 px (웹 · 공유)").tag(2048)
                                    Text("4000 px (고화질)").tag(4000)
                                }
                                Picker("화질", selection: $quality) {
                                    Text("보통 (80)").tag(80)
                                    Text("높음 (88)").tag(88)
                                    Text("최고 (95)").tag(95)
                                }
                            }
                        }

                        Section("파일 이름") {
                            Toggle("앞에 촬영 일시 붙이기", isOn: $dateNames)
                                .tint(palette.accent)
                        }

                        Section("요약") {
                            LabeledContent("선택한 사진", value: "\(plan.photos)장")
                            if mode == .originals {
                                LabeledContent("예상 크기", value: LoupeFormat.bytes(plan.originalsBytes))
                            } else {
                                LabeledContent("카메라 JPEG", value: LoupeFormat.bytes(plan.jpegBytes))
                            }
                            if plan.rawOnly > 0 && mode != .originals {
                                Text("JPEG 짝이 없는 RAW \(plan.rawOnly)장은 RAW 내부 원본 크기 JPEG로 변환합니다.")
                                    .font(.caption)
                                    .foregroundStyle(palette.text2)
                            }
                            if plan.framedCount > 0 {
                                Text(
                                    mode == .originals
                                        ? "크롭 · 수평이 적용된 \(plan.framedCount)장은 원본 그대로 내보냅니다."
                                        : "크롭 · 수평이 적용된 \(plan.framedCount)장은 자른 모양으로 내보냅니다."
                                )
                                .font(.caption)
                                .foregroundStyle(palette.accent)
                            }
                        }
                    }
                    .scrollContentBackground(.hidden)

                    Button("내보내기 · 공유하기") {
                        store.runExport(mode: mode, longEdge: longEdge, quality: quality, dateNames: dateNames)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(palette.accent)
                    .padding(.horizontal, 20)
                    .padding(.bottom, 12)

                case let .running(done, total):
                    VStack(spacing: 12) {
                        ProgressView(value: total > 0 ? Double(done) / Double(total) : 0)
                            .tint(palette.accent)
                        Text("내보내는 중… (\(done) / \(total))")
                            .font(.subheadline.monospacedDigit())
                            .foregroundStyle(palette.text)
                        Button("취소", role: .destructive) { store.cancelExport() }
                            .buttonStyle(.bordered)
                    }
                    .padding(24)
                    .frame(maxHeight: .infinity)

                case let .done(result, outDir):
                    VStack(spacing: 12) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 46))
                            .foregroundStyle(palette.success)
                        Text("\(result.files)개 파일 준비 완료")
                            .font(.headline)
                            .foregroundStyle(palette.text)
                        Text(LoupeFormat.bytes(result.bytes))
                            .font(.subheadline)
                            .foregroundStyle(palette.text2)
                        Button("AirDrop · 파일 · Lightroom 으로 보내기") {
                            let files = (try? FileManager.default.contentsOfDirectory(
                                at: outDir,
                                includingPropertiesForKeys: nil
                            )) ?? []
                            shareItems = files
                            sharePresented = !files.isEmpty
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(palette.accent)
                        .disabled(result.files == 0)
                    }
                    .padding(24)
                    .frame(maxHeight: .infinity)

                case .failed(let error):
                    VStack(spacing: 12) {
                        Text(error.title).font(.headline).foregroundStyle(palette.danger)
                        Text(error.message).font(.subheadline).foregroundStyle(palette.text2).multilineTextAlignment(.center)
                    }
                    .padding(24)
                    .frame(maxHeight: .infinity)
                }
            }
            .background(palette.content)
            .navigationTitle("사진 내보내기")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("닫기") { dismiss() }
                }
            }
            .sheet(isPresented: $sharePresented) {
                ActivityShareSheet(items: shareItems)
            }
            .onDisappear {
                if case let .done(_, outDir) = store.exportPhase {
                    try? FileManager.default.removeItem(at: outDir)
                }
            }
        }
    }
}

struct ActivityShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}
