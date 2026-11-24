#if DEBUG
import Foundation

/// Store screenshots on the iOS Simulator (`ios/Scripts/screenshots.sh`).
///
/// The script launches the Debug build with `-LoupeScene <scene>` (and `-LoupePhoto <name>`),
/// the app opens that screen by itself once the library is loaded, and the script captures it with
/// `simctl io screenshot`. No UI test has to tap through the app. Not compiled into Release
/// (App Store) builds.
enum ScreenshotScene: String {
    /// The grid of every photo.
    case library
    /// The viewer on `-LoupePhoto` (iPad: with the info panel).
    case viewer
    /// The viewer with the info panel (iPhone: the sheet).
    case info
    /// The viewer with the composition guide (G).
    case composition
    /// A컷 후보.
    case keepers
    /// B컷 후보.
    case candidates
    /// The import sheet after scanning the folder `Documents/<-LoupeCard>`.
    case importCard = "import"
    /// 설정.
    case settings

    static var current: ScreenshotScene? {
        UserDefaults.standard.string(forKey: "LoupeScene").flatMap(ScreenshotScene.init(rawValue:))
    }

    /// File name (or its start) of the photo the viewer scenes open, e.g. `DSC_1930`.
    static var photoName: String? {
        UserDefaults.standard.string(forKey: "LoupePhoto")
    }

    /// Folder in Documents the import scene scans, as if it were the SD card.
    static var cardFolder: String? {
        UserDefaults.standard.string(forKey: "LoupeCard")
    }
}

extension LibraryStore {
    /// Opens the screen asked for with `-LoupeScene` (nothing without it).
    func openScreenshotScene() async {
        guard let scene = ScreenshotScene.current else { return }
        suppressGuidesForScreenshots()
        switch scene {
        case .library:
            break
        case .keepers:
            selectView(.keepers)
        case .candidates:
            selectView(.candidates)
        case .viewer, .info, .composition:
            showCompositionOverlay = scene == .composition
            showInspector = true
            if let id = await screenshotPhotoID() {
                openViewer(at: id)
            }
        case .importCard:
            openImport()
            if let folder = ScreenshotScene.cardFolder,
               let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
                importer.scanCardFolder(url: docs.appendingPathComponent(folder, isDirectory: true))
            }
        case .settings:
            refreshTrashBytes()
            settingsSheetPresented = true
        }
    }

    private func screenshotPhotoID() async -> Int64? {
        let ids = visibleIDs
        guard let name = ScreenshotScene.photoName, let engine else { return ids.first }
        let batch = (try? await engine.photosBatch(ids: ids)) ?? []
        return batch.first(where: { $0.name.hasPrefix(name) })?.id ?? ids.first
    }
}
#endif
