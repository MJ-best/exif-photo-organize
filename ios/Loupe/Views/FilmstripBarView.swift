import SwiftUI
import UIKit

/// Compact scrollable thumbnail strip placed strictly BELOW the viewer canvas (never overlapping
/// the photo). Shares `ImagePipeline`'s coalesced thumbnail cache with the grid for zero extra FFI
/// decoding and highlights the active frame, pick/reject flags, and star badges.
struct FilmstripBarView: View {
    let ids: [Int64]
    let currentIndex: Int
    let summaries: [Int64: LoupePhotoSummary]
    let reloadToken: Int
    let pipeline: ImagePipeline
    let onSelectIndex: (Int) -> Void

    @Environment(\.loupePalette) private var palette

    private var currentID: Int64? {
        guard currentIndex >= 0 && currentIndex < ids.count else { return nil }
        return ids[currentIndex]
    }

    var body: some View {
        VStack(spacing: 0) {
            Divider().overlay(palette.separator)
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(spacing: 5) {
                        ForEach(ids, id: \.self) { id in
                            FilmstripThumbCell(
                                photoID: id,
                                summary: summaries[id],
                                isCurrent: id == currentID,
                                reloadToken: reloadToken,
                                pipeline: pipeline
                            )
                            .id(id)
                            .onTapGesture {
                                if let idx = ids.firstIndex(of: id) {
                                    onSelectIndex(idx)
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                }
                .onAppear {
                    if let target = currentID {
                        proxy.scrollTo(target, anchor: .center)
                    }
                }
                .onChange(of: currentIndex) { _, _ in
                    if let target = currentID {
                        withAnimation(.easeInOut(duration: 0.16)) {
                            proxy.scrollTo(target, anchor: .center)
                        }
                    }
                }
            }
            .frame(height: 54)
            .background(palette.panel)
        }
    }
}

private struct FilmstripThumbCell: View {
    let photoID: Int64
    let summary: LoupePhotoSummary?
    let isCurrent: Bool
    let reloadToken: Int
    let pipeline: ImagePipeline

    @State private var image: UIImage?
    @Environment(\.loupePalette) private var palette

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            palette.placeholder

            if let image {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 42, height: 42)
                    .clipped()
            }

            if let summary, summary.flag != 0 || summary.rating > 0 {
                HStack(spacing: 2) {
                    if summary.rating > 0 {
                        Circle()
                            .fill(palette.star)
                            .frame(width: 6, height: 6)
                    }
                    if summary.flag != 0 {
                        Circle()
                            .fill(summary.flag > 0 ? palette.success : palette.danger)
                            .frame(width: 6, height: 6)
                    }
                }
                .padding(3)
            }
        }
        .frame(width: 42, height: 42)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay {
            if isCurrent {
                RoundedRectangle(cornerRadius: 6)
                    .stroke(palette.accent, lineWidth: 2.5)
            } else {
                RoundedRectangle(cornerRadius: 6)
                    .stroke(palette.separator.opacity(0.6), lineWidth: 0.75)
            }
        }
        .opacity(isCurrent ? 1.0 : 0.78)
        .task(id: "\(photoID)_\(reloadToken)") {
            image = pipeline.cachedThumbnail(id: photoID)
            if image == nil {
                image = await pipeline.thumbnail(id: photoID, targetPixelSize: 160)
            }
        }
    }
}
