# LOUPE iOS / iPadOS 앱 (`ios/`)

iPhone · iPad(iOS 17 / iPadOS 17+) 네이티브 SwiftUI 앱 소스입니다.
상세한 아키텍처 설계와 애플 · 당근 · 토스 관점의 리뷰 포인트 대응 내역은 [docs/ios-app-store/README.md](../docs/ios-app-store/README.md) 를 참고하세요.

## 구조

- `Loupe/App/` — 앱 진입점 (`LoupeApp.swift`: `scenePhase` suspend/resume, 메모리 경고, 충전 중 `BGProcessingTask` + `TaskCompletionGate`)
- `Loupe/Engine/` — Anti-Corruption Layer (`PhotoEngineProtocol.swift`), UniFFI 브릿지 어댑터 (`RustPhotoEngine.swift`), 테스트·프리뷰용 `MockPhotoEngine.swift`
- `Loupe/Services/` — Zero-Copy mmap + Coalescer + ImageIO/AVFoundation 파이프라인 (`ImagePipeline.swift`), 백그라운드 작업·자동 잠금 가드 (`BackgroundTaskGuard.swift`), 열·RAM 기반 스레드 예산 (`ThermalBudgetMonitor.swift`), Security-Scoped Bookmark + iCloud 백업 제외 (`BookmarkStore.swift`), `OSLog` · `OSSignposter` (`Telemetry.swift`)
- `Loupe/Store/` — `@MainActor @Observable` 단방향 상태 머신 + 20단계 직렬화 Undo (`LibraryStore.swift`, `StoreTypes.swift`) 및 도메인 코디네이터 (`ImportCoordinator.swift`, `ExportCoordinator.swift`, `AnalysisCoordinator.swift`)
- `Loupe/Theme/` — 크림 · 코코아 테마 팔레트 (`LoupeTheme.swift`)
- `Loupe/Views/` — iPad `NavigationSplitView` · iPhone 컴팩트 리본, 윈도우 그리드, 확대 유지 뷰어 + 하단 필름스트립(`FilmstripBarView.swift`) + 키보드 단축키(`F1~F5`, `⌘Z` 등), `G` 구도 오버레이, `0.7.4` B컷↔A컷 스펙트럼 게이지, SD 가져오기, AirDrop/Files/Lightroom 내보내기, 설정(WAL 최적화 · 스레드 예산 표시)
- `LoupeTests/` — `XCTest` 단위 테스트 (`LibraryStoreTests`, `ThermalAndPipelineTests`)
- `Scripts/build-rust-ios.sh` — `crates/ios` 정적 라이브러리 빌드 → Swift UniFFI 바인딩 생성 → `LoupeCore.xcframework` 패키징

## 빌드 (macOS + Xcode 16+)

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
brew install xcodegen

./ios/Scripts/build-rust-ios.sh --sim-only --debug
cd ios && xcodegen generate && open Loupe.xcodeproj
```
