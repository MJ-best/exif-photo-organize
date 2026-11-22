import SwiftUI
import UIKit

/// Single grid tile with cooperative Swift Concurrency thumbnail cancellation and minimal,
/// non-overlapping corner badges.
struct PhotoCellView: View {
    let photoID: Int64
    let summary: LoupePhotoSummary?
    let selected: Bool
    let isSelectionMode: Bool
    let threshold: Int
    let reloadToken: Int
    let pipeline: ImagePipeline

    @State private var image: UIImage?
    @Environment(\.loupePalette) private var palette

    var body: some View {
        ZStack {
            palette.placeholder

            if let image {
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
                    .clipped()
            }

            if let summary {
                badgesOverlay(summary)
            }

            if selected {
                RoundedRectangle(cornerRadius: 8)
                    .stroke(palette.accent, lineWidth: 3)
            } else if let flag = summary?.flag, flag != 0 {
                RoundedRectangle(cornerRadius: 8)
                    .stroke(flag > 0 ? palette.success : palette.danger, lineWidth: 2)
            }
        }
        .aspectRatio(1, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .contentShape(RoundedRectangle(cornerRadius: 8))
        .task(id: "\(photoID)_\(reloadToken)") {
            image = pipeline.cachedThumbnail(id: photoID)
            if image == nil {
                image = await pipeline.thumbnail(id: photoID, targetPixelSize: 480)
            }
        }
    }

    private func badgesOverlay(_ summary: LoupePhotoSummary) -> some View {
        VStack {
            HStack(alignment: .top, spacing: 4) {
                if let badge = summary.formatBadge {
                    pill(badge, bg: palette.hud, fg: .white)
                }
                if summary.burstSize > 1 {
                    pill(
                        "\(summary.burstRank)/\(summary.burstSize)",
                        bg: summary.burstRank == 1 ? palette.success.opacity(0.92) : palette.hud,
                        fg: .white
                    )
                }
                Spacer(minLength: 0)
                if isSelectionMode {
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(selected ? palette.accent : .white)
                        .shadow(color: .black.opacity(0.45), radius: 2)
                } else if summary.flag != 0 {
                    Image(systemName: summary.flag > 0 ? "flag.fill" : "xmark.circle.fill")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(summary.flag > 0 ? palette.success : palette.danger)
                        .padding(4)
                        .background(.black.opacity(0.45), in: Circle())
                }
            }

            Spacer(minLength: 0)

            HStack(alignment: .bottom, spacing: 4) {
                if summary.rating > 0 {
                    pill(String(repeating: "★", count: summary.rating), bg: palette.hud, fg: palette.star)
                }
                Spacer(minLength: 0)
                if summary.hasFraming {
                    Image(systemName: "crop")
                        .font(.system(size: 10, weight: .bold))
                        .foregroundStyle(.white)
                        .padding(4)
                        .background(palette.accent.opacity(0.9), in: Circle())
                }
                if let bPct = summary.bPct, bPct >= threshold, summary.flag == 0, summary.rating == 0 {
                    pill("B \(bPct)%", bg: palette.warning.opacity(0.92), fg: .white)
                } else if let top = summary.topPct, top <= 15, summary.flag == 0, summary.rating == 0 {
                    pill("A \(top)%", bg: palette.success.opacity(0.92), fg: .white)
                }
            }
        }
        .padding(6)
    }

    private func pill(_ text: String, bg: Color, fg: Color) -> some View {
        Text(text)
            .font(.system(size: 10, weight: .semibold))
            .foregroundStyle(fg)
            .lineLimit(1)
            .padding(.horizontal, 5)
            .padding(.vertical, 2)
            .background(bg, in: Capsule())
    }
}
