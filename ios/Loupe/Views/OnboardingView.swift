import SwiftUI
import UIKit

/// First-run 3-step welcome onboarding establishing the LOUPE iPhone/iPad mental model:
/// 1. 3-step SD card -> LOUPE -> AI culling -> A-cut export flow
/// 2. Choosing the LOUPE library space (on-device folder vs external USB-C SSD)
/// 3. Interactive 3-gesture culling preview (swipe up = pick, swipe down = reject, double tap = zoom)
struct OnboardingView: View {
    @Bindable var store: LibraryStore
    @State private var page: Int = 0
    @State private var selectedStorage: StorageChoice = .onDevice
    @State private var gesturePreview: GestureDemoState = .none
    @State private var ssdPickerPresented: Bool = false

    @Environment(\.loupePalette) private var palette
    @Environment(\.dismiss) private var dismiss

    private enum StorageChoice {
        case onDevice
        case externalSSD
    }

    private enum GestureDemoState {
        case none
        case pick
        case reject
        case zoom
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                topProgressBar

                TabView(selection: $page) {
                    flowOverviewPage.tag(0)
                    storageSelectionPage.tag(1)
                    gesturePreviewPage.tag(2)
                }
                .tabViewStyle(.page(indexDisplayMode: .never))
                .animation(.easeInOut(duration: 0.22), value: page)

                bottomActions
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(palette.content)
            .onAppear {
                selectedStorage = store.isExternalLibrary ? .externalSSD : .onDevice
            }
            .sheet(isPresented: $ssdPickerPresented) {
                FolderDocumentPicker { url in
                    Task { @MainActor in
                        await store.switchToExternalLibrary(url: url)
                        selectedStorage = .externalSSD
                    }
                }
            }
        }
    }

    // MARK: - Top Progress Bar

    private var topProgressBar: some View {
        HStack {
            HStack(spacing: 6) {
                ForEach(0..<3, id: \.self) { index in
                    Capsule()
                        .fill(index == page ? palette.accent : palette.control)
                        .frame(width: index == page ? 26 : 8, height: 6)
                        .animation(.easeInOut(duration: 0.2), value: page)
                }
            }
            Spacer()
            Button("건너뛰기") {
                store.completeOnboarding()
                dismiss()
            }
            .font(.subheadline.weight(.medium))
            .foregroundStyle(palette.text2)
        }
        .padding(.horizontal, 20)
        .padding(.top, 16)
        .padding(.bottom, 8)
    }

    // MARK: - Page 1: 3-Step Flow Overview

    private var flowOverviewPage: some View {
        ScrollView {
            VStack(spacing: 18) {
                Image("Mascot")
                    .resizable()
                    .scaledToFit()
                    .frame(width: 96, height: 96)
                    .padding(.top, 8)

                VStack(spacing: 6) {
                    Text("카메라 사진, 3단계로 정리 끝")
                        .font(.title3.weight(.bold))
                        .foregroundStyle(palette.text)
                        .multilineTextAlignment(.center)
                    Text("복잡한 설정 없이 SD 카드에서 바로 A컷만 골라내세요.")
                        .font(.subheadline)
                        .foregroundStyle(palette.text2)
                        .multilineTextAlignment(.center)
                }

                VStack(spacing: 10) {
                    stepCard(
                        number: "1",
                        title: "SD 카드 연결 · 가져오기",
                        detail: "USB-C 카드 리더를 꽂으면 날짜별로 복사하고 RAW+JPEG를 한 장으로 묶습니다."
                    )
                    stepCard(
                        number: "2",
                        title: "AI가 B컷 · A컷 자동 분류",
                        detail: "초점 흐림 · 눈 감김 · 구도를 기기 안에서 오프라인으로 분석합니다."
                    )
                    stepCard(
                        number: "3",
                        title: "A컷만 사진 앱 · 맥으로",
                        detail: "고른 사진만 사진 앱, AirDrop, 파일, Lightroom으로 바로 내보냅니다."
                    )
                }

                Label("모든 사진과 AI 분석은 기기 안에서만 처리 (전송 0)", systemImage: "lock.shield")
                    .font(.caption)
                    .foregroundStyle(palette.text3)
                    .padding(.top, 4)
            }
            .frame(maxWidth: 460)
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity)
        }
    }

    // MARK: - Page 2: Storage Location Mental Model

    private var storageSelectionPage: some View {
        ScrollView {
            VStack(spacing: 16) {
                VStack(spacing: 6) {
                    Text("LOUPE 앱 공간 선택")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(palette.accent)
                    Text("가져온 사진은 어디에 둘까요?")
                        .font(.title3.weight(.bold))
                        .foregroundStyle(palette.text)
                        .multilineTextAlignment(.center)
                    Text("나중에 설정에서 언제든 바꿀 수 있습니다.")
                        .font(.subheadline)
                        .foregroundStyle(palette.text2)
                }
                .padding(.top, 12)

                storageCard(
                    choice: .onDevice,
                    icon: "iphone",
                    title: "이 기기 (LOUPE 앱 공간)",
                    badge: "추천",
                    badgeHighlighted: true,
                    description: "외장 SSD 없이 바로 시작합니다. 「파일」 앱 → 나의 기기 → LOUPE 폴더에 날짜별로 저장됩니다.",
                    footnote: "앱을 삭제하면 함께 지워지므로, 셀렉 후 A컷은 사진 앱이나 맥으로 내보내세요."
                ) {
                    selectedStorage = .onDevice
                    if store.isExternalLibrary {
                        Task { await store.switchToDefaultLibrary() }
                    }
                }

                storageCard(
                    choice: .externalSSD,
                    icon: "externaldrive.fill",
                    title: "외장 SSD 폴더",
                    badge: "맥 연동",
                    badgeHighlighted: false,
                    description: "USB-C 외장 SSD에 직접 저장합니다. 맥·윈도우 LOUPE에서 꽂기만 하면 별점과 분류가 그대로 이어집니다.",
                    footnote: store.isExternalLibrary ? "현재 연결됨: \(store.libraryDisplayName)" : "외장 SSD가 연결되어 있다면 아래 버튼으로 폴더를 고를 수 있습니다."
                ) {
                    selectedStorage = .externalSSD
                }

                if selectedStorage == .externalSSD {
                    Button {
                        ssdPickerPresented = true
                    } label: {
                        Label(
                            store.isExternalLibrary ? "다른 외장 SSD 폴더 고르기…" : "지금 외장 SSD 폴더 고르기…",
                            systemImage: "folder.badge.plus"
                        )
                        .font(.subheadline.weight(.semibold))
                        .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                    .tint(palette.accent)
                }
            }
            .frame(maxWidth: 460)
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity)
        }
    }

    // MARK: - Page 3: Interactive 3-Gesture Preview

    private var gesturePreviewPage: some View {
        ScrollView {
            VStack(spacing: 16) {
                VStack(spacing: 6) {
                    Text("빠른 셀렉 조작법")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(palette.accent)
                    Text("손가락 3가지면 충분합니다")
                        .font(.title3.weight(.bold))
                        .foregroundStyle(palette.text)
                        .multilineTextAlignment(.center)
                    Text("아래 예시 카드를 직접 밀거나 버튼을 눌러 확인해 보세요.")
                        .font(.subheadline)
                        .foregroundStyle(palette.text2)
                        .multilineTextAlignment(.center)
                }
                .padding(.top, 12)

                VStack(spacing: 10) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 16)
                            .fill(
                                LinearGradient(
                                    colors: [Color(hex: 0x3E4D5C), Color(hex: 0x9B7E68)],
                                    startPoint: .topLeading,
                                    endPoint: .bottomTrailing
                                )
                            )
                            .scaleEffect(gesturePreview == .zoom ? 1.18 : 1.0)
                            .animation(.spring(response: 0.26, dampingFraction: 0.8), value: gesturePreview)

                        VStack(spacing: 4) {
                            Image(systemName: "camera.aperture")
                                .font(.title)
                                .foregroundStyle(.white.opacity(0.9))
                            Text("DSC_1930.NEF")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.white.opacity(0.95))
                            Text("50mm · ƒ/4 · 1/1000s")
                                .font(.caption2)
                                .foregroundStyle(.white.opacity(0.75))
                        }
                    }
                    .frame(height: 150)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) {
                        triggerDemo(gesturePreview == .zoom ? .none : .zoom)
                    }
                    .simultaneousGesture(
                        DragGesture(minimumDistance: 24)
                            .onEnded { value in
                                let dx = value.translation.width
                                let dy = value.translation.height
                                guard abs(dy) > abs(dx) else { return }
                                if dy < -28 {
                                    triggerDemo(.pick)
                                } else if dy > 28 {
                                    triggerDemo(.reject)
                                }
                            }
                    )

                    gestureDemoFeedback
                        .frame(height: 26)
                }
                .padding(12)
                .background(palette.panel, in: RoundedRectangle(cornerRadius: 18))
                .overlay(RoundedRectangle(cornerRadius: 18).stroke(palette.separator, lineWidth: 1))

                HStack(spacing: 10) {
                    gestureTile(symbol: "arrow.up", color: palette.success, title: "위로 밀기", subtitle: "픽 (A컷)", active: gesturePreview == .pick) {
                        triggerDemo(.pick)
                    }
                    gestureTile(symbol: "arrow.down", color: palette.danger, title: "아래로 밀기", subtitle: "거절 (B컷)", active: gesturePreview == .reject) {
                        triggerDemo(.reject)
                    }
                    gestureTile(symbol: "plus.magnifyingglass", color: palette.accent, title: "두 번 탭", subtitle: "초점 확대", active: gesturePreview == .zoom) {
                        triggerDemo(gesturePreview == .zoom ? .none : .zoom)
                    }
                }
            }
            .frame(maxWidth: 460)
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity)
        }
    }

    @ViewBuilder
    private var gestureDemoFeedback: some View {
        switch gesturePreview {
        case .pick:
            Label("픽 (A컷) 선택됨!", systemImage: "flag.fill")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
                .background(palette.success, in: Capsule())
        case .reject:
            Label("거절 (B컷) 선택됨!", systemImage: "xmark.circle.fill")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
                .background(palette.danger, in: Capsule())
        case .zoom:
            Label("100% 초점 확대 유지!", systemImage: "magnifyingglass")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
                .background(palette.accent, in: Capsule())
        case .none:
            Text("카드를 위·아래로 밀거나 두 번 탭해 보세요")
                .font(.caption)
                .foregroundStyle(palette.text3)
        }
    }

    // MARK: - Bottom CTA Bar

    private var bottomActions: some View {
        VStack(spacing: 10) {
            Divider().overlay(palette.separator)

            VStack(spacing: 10) {
                if page < 2 {
                    Button {
                        withAnimation { page += 1 }
                    } label: {
                        Text("다음 (\(page + 1)/3)")
                            .font(.body.weight(.semibold))
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 4)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(palette.accent)
                    .controlSize(.large)
                } else {
                    Button {
                        store.completeOnboarding()
                        dismiss()
                    } label: {
                        Text("LOUPE 시작하기")
                            .font(.body.weight(.semibold))
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 4)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(palette.accent)
                    .controlSize(.large)

                    Button {
                        Task {
                            await store.enterSampleMode()
                            dismiss()
                        }
                    } label: {
                        if store.isLoadingSampleMode {
                            HStack(spacing: 8) {
                                ProgressView().controlSize(.small)
                                Text("견본 사진 불러오는 중…")
                            }
                            .font(.subheadline.weight(.semibold))
                        } else {
                            Text("SD 카드 없이 견본 사진 5장으로 체험해 보기")
                                .font(.subheadline.weight(.semibold))
                        }
                    }
                    .foregroundStyle(palette.accent)
                    .disabled(store.isLoadingSampleMode)
                    .padding(.vertical, 2)
                }
            }
            .frame(maxWidth: 460)
            .padding(.horizontal, 20)
            .padding(.top, 10)
            .padding(.bottom, 16)
        }
        .background(palette.toolbar)
    }

    // MARK: - Reusable Subviews

    private func stepCard(number: String, title: String, detail: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(number)
                .font(.caption.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 24, height: 24)
                .background(palette.accent, in: Circle())

            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.subheadline.weight(.bold))
                    .foregroundStyle(palette.text)
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(palette.text2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(palette.panel, in: RoundedRectangle(cornerRadius: 16))
        .overlay(RoundedRectangle(cornerRadius: 16).stroke(palette.separator, lineWidth: 1))
    }

    private func storageCard(
        choice: StorageChoice,
        icon: String,
        title: String,
        badge: String,
        badgeHighlighted: Bool,
        description: String,
        footnote: String,
        action: @escaping () -> Void
    ) -> some View {
        let isSelected = selectedStorage == choice
        return Button(action: action) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Image(systemName: icon)
                        .font(.body.weight(.semibold))
                        .foregroundStyle(isSelected ? palette.accent : palette.text2)
                    Text(title)
                        .font(.subheadline.weight(.bold))
                        .foregroundStyle(palette.text)
                    Spacer()
                    Text(badge)
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(badgeHighlighted ? palette.selected : palette.control, in: Capsule())
                        .foregroundStyle(badgeHighlighted ? palette.accent : palette.text2)
                }

                Text(description)
                    .font(.caption)
                    .foregroundStyle(palette.text2)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)

                Text(footnote)
                    .font(.caption2)
                    .foregroundStyle(palette.text3)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(15)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(isSelected ? palette.panel : palette.content, in: RoundedRectangle(cornerRadius: 16))
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .stroke(isSelected ? palette.accent : palette.separator, lineWidth: isSelected ? 2 : 1)
            )
        }
        .buttonStyle(.plain)
    }

    private func gestureTile(
        symbol: String,
        color: Color,
        title: String,
        subtitle: String,
        active: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            VStack(spacing: 5) {
                Image(systemName: symbol)
                    .font(.headline.weight(.bold))
                    .foregroundStyle(color)
                Text(title)
                    .font(.caption.weight(.bold))
                    .foregroundStyle(palette.text)
                    .lineLimit(1)
                Text(subtitle)
                    .font(.caption2)
                    .foregroundStyle(palette.text2)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
            .padding(.horizontal, 6)
            .background(active ? palette.selected : palette.panel, in: RoundedRectangle(cornerRadius: 14))
            .overlay(
                RoundedRectangle(cornerRadius: 14)
                    .stroke(active ? palette.accent : palette.separator, lineWidth: active ? 1.5 : 1)
            )
        }
        .buttonStyle(.plain)
    }

    private func triggerDemo(_ next: GestureDemoState) {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        withAnimation(.easeInOut(duration: 0.16)) {
            gesturePreview = next
        }
    }
}
