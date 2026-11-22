import Foundation

/// RAII handle guaranteeing `stopAccessingSecurityScopedResource()` is always paired with
/// `startAccessingSecurityScopedResource()` for iOS external SSD / SD card folders.
final class SecurityScopedAccess: @unchecked Sendable {
    let url: URL
    private let active: Bool

    init(url: URL) {
        self.url = url
        self.active = url.startAccessingSecurityScopedResource()
        if !active {
            LoupeLog.bookmark.debug("startAccessingSecurityScopedResource returned false for \(url.lastPathComponent)")
        }
    }

    deinit {
        if active {
            url.stopAccessingSecurityScopedResource()
        }
    }
}

/// Persists user-chosen external library bookmarks (USB-C SSD / Files folder) and ensures the
/// default on-device library excludes RAW/thumbnails from iCloud Backup (App Review 2.23).
final class BookmarkStore: Sendable {
    private static let externalLibraryBookmarkKey = "loupe.external_library_bookmark"

    /// Default on-device library directory (`Documents/LOUPE`), marked `isExcludedFromBackup = true`
    /// so tens of gigabytes of RAW photos never blow up the user's iCloud backup quota.
    func defaultLibraryURL() throws -> URL {
        let docs = try FileManager.default.url(
            for: .documentDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        let dir = docs.appendingPathComponent("LOUPE", isDirectory: true)
        if !FileManager.default.fileExists(atPath: dir.path) {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        markExcludedFromBackup(dir)
        return dir
    }

    /// Marks `url` as excluded from iCloud Backup (App Store Guideline 2.23 compliance).
    func markExcludedFromBackup(_ url: URL) {
        var target = url
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        do {
            try target.setResourceValues(values)
        } catch {
            LoupeLog.bookmark.error("Failed to set isExcludedFromBackup: \(error.localizedDescription)")
        }
    }

    /// Saves a security-scoped bookmark to an external SSD or Files.app library folder chosen by
    /// the user via `UIDocumentPickerViewController`.
    /// Note: iOS uses `.minimalBookmark` (`.withSecurityScope` is macOS-only).
    func saveExternalLibrary(_ url: URL) throws {
        let access = SecurityScopedAccess(url: url)
        let data = try withExtendedLifetime(access) {
            try url.bookmarkData(
                options: .minimalBookmark,
                includingResourceValuesForKeys: nil,
                relativeTo: nil
            )
        }
        UserDefaults.standard.set(data, forKey: Self.externalLibraryBookmarkKey)
        LoupeLog.bookmark.info("Saved external library bookmark: \(url.lastPathComponent)")
    }

    func clearExternalLibrary() {
        UserDefaults.standard.removeObject(forKey: Self.externalLibraryBookmarkKey)
    }

    /// Resolves a previously saved external library bookmark, refreshing stale bookmark data if
    /// needed. Returns `nil` if the user uses the default on-device library or the drive is absent.
    func resolveExternalLibrary() -> SecurityScopedAccess? {
        guard let data = UserDefaults.standard.data(forKey: Self.externalLibraryBookmarkKey) else {
            return nil
        }
        var stale = false
        do {
            let url = try URL(
                resolvingBookmarkData: data,
                options: [],
                relativeTo: nil,
                bookmarkDataIsStale: &stale
            )
            let access = SecurityScopedAccess(url: url)
            if stale {
                if let refreshed = try? url.bookmarkData(
                    options: .minimalBookmark,
                    includingResourceValuesForKeys: nil,
                    relativeTo: nil
                ) {
                    UserDefaults.standard.set(refreshed, forKey: Self.externalLibraryBookmarkKey)
                }
            }
            guard FileManager.default.fileExists(atPath: url.path) else {
                return nil
            }
            return access
        } catch {
            LoupeLog.bookmark.warning("Could not resolve external library bookmark: \(error.localizedDescription)")
            return nil
        }
    }

    /// Queries available capacity (bytes) for preflight import storage checks.
    static func availableCapacityBytes(at url: URL) -> Int64 {
        let keys: Set<URLResourceKey> = [
            .volumeAvailableCapacityForImportantUsageKey,
            .volumeAvailableCapacityKey,
        ]
        guard let values = try? url.resourceValues(forKeys: keys) else {
            return -1
        }
        if let important = values.volumeAvailableCapacityForImportantUsage {
            return important
        }
        if let basic = values.volumeAvailableCapacity {
            return Int64(basic)
        }
        return -1
    }
}
