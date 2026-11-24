import SwiftUI

/// Always-accessible `?` quick guide covering storage locations, SD card import instructions,
/// viewer touch & iPad keyboard shortcuts, culling terminology, and sample tryout controls.
struct GuideSheetView: View {
    @Bindable var store: LibraryStore
    @Environment(\.loupePalette) private var palette
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("1. 사진은 어디에 저장되나요?") {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 8) {
                            Image(systemName: store.isSampleMode ? "sparkles" : (store.isExternalLibrary ? "externaldrive.fill" : "iphone"))
                                .foregroundStyle(palette.accent)
                            Text("현재 위치: \(store.libraryDisplayName)")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(palette.text)
                        }
                        Text("기본값은 「파일」 앱 → 나의 기기 → LOUPE 폴더입니다. 설정에서 USB-C 외장 SSD 폴더로 바꾸면 맥·윈도우 LOUPE와 별점·분류를 그대로 이어 쓸 수 있습니다.")
                            .font(.caption)
                            .foregroundStyle(palette.text2)
                    }
                    .padding(.vertical, 2)

                    Button("외장 SSD 폴더 바로 열기…") {
                        store.openExternalLibraryPickerAfterSheetDismiss()
                    }
                    .foregroundStyle(palette.accent)
                }

                Section("2. SD 카드 가져오는 법") {
                    guideBullet("1", "USB-C 카드 리더로 카메라 SD 카드를 연결합니다.")
                    guideBullet("2", "가져오기 버튼을 눌러 파일 앱이 열리면 카드 이름 → DCIM 폴더를 열고 우측 상단 「열기」를 누릅니다.")
                    guideBullet("3", "EXIF 날짜별로 복사되며 RAW+JPEG는 한 장으로 묶이고, 이미 가져온 사진은 자동으로 건너뜁니다.")
                }

                Section("3. 뷰어 제스처") {
                    shortcutRow("↑ 위로 밀기", "픽 (A컷으로 표시)")
                    shortcutRow("↓ 아래로 밀기", "거절 (B컷으로 표시)")
                    shortcutRow("← → 좌우로 밀기", "이전 · 다음 사진 이동")
                    shortcutRow("두 번 탭", "100% 초점 확대 · 원래대로 (확대 위치 유지)")
                }

                Section("iPad 키보드 단축키") {
                    shortcutRow("1 ~ 5 / 0", "별점 주기 / 별점 해제")
                    shortcutRow("P / X / U", "픽 / 거절 / 표시 해제")
                    shortcutRow("Space / 두 번", "다음 사진 / 100% 초점 확대")
                    shortcutRow("Z / G / R", "확대 / 구도 가이드 / 자동 수평")
                    shortcutRow("⌘Z / Delete", "되돌리기 / 휴지통으로 옮기기")
                }

                Section("4. 용어 한눈에 보기") {
                    termRow("미결정", "아직 픽 · 거절 · 별점을 주지 않은 사진")
                    termRow("B컷 후보", "AI가 초점 흐림 · 눈 감김 · 흔들림 등으로 찾은 걸러낼 사진")
                    termRow("A컷 후보", "선명도 · 구도 점수가 높은 상위 추천 사진")
                    termRow("픽 / 거절", "내가 직접 확정한 남길 사진 / 지울 사진")
                }

                Section("다시 보기 · 체험") {
                    Button {
                        store.presentOnboardingReplay()
                    } label: {
                        Label("처음 웰컴 가이드 다시 보기", systemImage: "book.pages")
                    }

                    if store.isSampleMode {
                        Button(role: .destructive) {
                            dismiss()
                            Task { await store.exitSampleMode() }
                        } label: {
                            Label("견본 체험 끝내고 내 라이브러리로 돌아가기", systemImage: "arrow.uturn.left")
                        }
                    } else {
                        Button {
                            dismiss()
                            Task { await store.enterSampleMode() }
                        } label: {
                            Label("견본 사진 5장으로 체험해 보기", systemImage: "photo.on.rectangle.angled")
                                .foregroundStyle(palette.accent)
                        }
                        .disabled(store.isLoadingSampleMode)
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(palette.content)
            .navigationTitle("LOUPE 사용 가이드")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("닫기") { dismiss() }
                }
            }
        }
    }

    private func guideBullet(_ num: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Text(num)
                .font(.caption2.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 20, height: 20)
                .background(palette.accent, in: Circle())
            Text(text)
                .font(.caption)
                .foregroundStyle(palette.text)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 2)
    }

    private func shortcutRow(_ key: String, _ desc: String) -> some View {
        HStack {
            Text(key)
                .font(.caption.weight(.semibold).monospaced())
                .foregroundStyle(palette.text)
            Spacer()
            Text(desc)
                .font(.caption)
                .foregroundStyle(palette.text2)
                .multilineTextAlignment(.trailing)
        }
    }

    private func termRow(_ term: String, _ desc: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(term)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(palette.text)
            Text(desc)
                .font(.caption)
                .foregroundStyle(palette.text2)
        }
        .padding(.vertical, 2)
    }
}
