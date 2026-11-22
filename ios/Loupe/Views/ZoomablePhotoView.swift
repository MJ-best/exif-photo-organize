import SwiftUI
import UIKit

/// Zoomable photo canvas that shares `scale` and `offset` across photos so photographers can
/// keep 100% focus inspection zoom locked while stepping through burst sequences (v0.6.1).
/// At 1x scale, vertical swipes trigger Pick (up) and Reject (down); when zoomed, drag pans.
struct ZoomablePhotoView: View {
    let photoID: Int64
    let maxEdge: Int
    let reloadToken: Int
    let pipeline: ImagePipeline
    let overlay: LoupeCompositionOverlay?
    let showOverlay: Bool
    @Binding var scale: CGFloat
    @Binding var offset: CGSize
    let onSwipeUpPick: () -> Void
    let onSwipeDownReject: () -> Void
    let onSwipeLeftNext: () -> Void
    let onSwipeRightPrev: () -> Void

    @State private var image: UIImage?
    @State private var thumbFallback: UIImage?
    @GestureState private var gestureScale: CGFloat = 1.0
    @GestureState private var dragDelta: CGSize = .zero
    @GestureState private var swipePreviewOffset: CGSize = .zero
    @Environment(\.loupePalette) private var palette

    var body: some View {
        GeometryReader { geo in
            let activeImage = image ?? thumbFallback
            let fitted = fittedSize(for: activeImage?.size, in: geo.size)
            let effectiveScale = max(1.0, min(6.0, scale * gestureScale))
            let effectiveOffset: CGSize = scale > 1.01
                ? clampedOffset(
                    CGSize(width: offset.width + dragDelta.width,
                           height: offset.height + dragDelta.height),
                    fitted: fitted,
                    container: geo.size,
                    scale: effectiveScale
                )
                : swipePreviewOffset

            ZStack {
                palette.viewer

                if let display = activeImage {
                    ZStack {
                        Image(uiImage: display)
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                            .frame(width: fitted.width, height: fitted.height)

                        if showOverlay, let overlay {
                            CompositionOverlayView(overlay: overlay)
                                .frame(width: fitted.width, height: fitted.height)
                        }
                    }
                    .scaleEffect(effectiveScale)
                    .offset(effectiveOffset)
                    .animation(.interactiveSpring(response: 0.22, dampingFraction: 0.86), value: scale)
                    .animation(.interactiveSpring(response: 0.2, dampingFraction: 0.88), value: swipePreviewOffset == .zero)
                } else {
                    ProgressView()
                        .tint(palette.accent)
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .clipped()
            .contentShape(Rectangle())
            .onTapGesture(count: 2) {
                withAnimation(.easeInOut(duration: 0.18)) {
                    if scale > 1.05 {
                        scale = 1.0
                        offset = .zero
                    } else {
                        scale = 2.5
                        offset = .zero
                    }
                }
            }
            .gesture(
                MagnifyGesture()
                    .updating($gestureScale) { value, state, _ in
                        state = value.magnification
                    }
                    .onEnded { value in
                        let next = max(1.0, min(6.0, scale * value.magnification))
                        scale = next < 1.05 ? 1.0 : next
                        if scale <= 1.0 {
                            offset = .zero
                        } else {
                            offset = clampedOffset(offset, fitted: fitted, container: geo.size, scale: scale)
                        }
                    }
            )
            .simultaneousGesture(
                DragGesture(minimumDistance: 14)
                    .updating($dragDelta) { value, state, _ in
                        if scale > 1.01 {
                            state = value.translation
                        }
                    }
                    .updating($swipePreviewOffset) { value, state, _ in
                        guard scale <= 1.01 else {
                            state = .zero
                            return
                        }
                        let dx = value.translation.width
                        let dy = value.translation.height
                        if abs(dx) >= abs(dy) {
                            state = CGSize(width: max(-120, min(120, dx * 0.45)), height: 0)
                        } else {
                            state = CGSize(width: 0, height: max(-64, min(64, dy * 0.28)))
                        }
                    }
                    .onEnded { value in
                        if scale > 1.01 {
                            let combined = CGSize(
                                width: offset.width + value.translation.width,
                                height: offset.height + value.translation.height
                            )
                            offset = clampedOffset(combined, fitted: fitted, container: geo.size, scale: scale)
                        } else {
                            let dx = value.translation.width
                            let dy = value.translation.height
                            if abs(dy) > abs(dx) && abs(dy) > 56 {
                                if dy < 0 {
                                    onSwipeUpPick()
                                } else {
                                    onSwipeDownReject()
                                }
                            } else if abs(dx) > abs(dy) && abs(dx) > 48 {
                                if dx < 0 {
                                    onSwipeLeftNext()
                                } else {
                                    onSwipeRightPrev()
                                }
                            }
                        }
                    }
            )
        }
        .task(id: "\(photoID)_\(maxEdge)_\(reloadToken)") {
            thumbFallback = pipeline.cachedThumbnail(id: photoID)
            image = pipeline.cachedPreview(id: photoID, maxEdge: maxEdge)
            if image == nil {
                if thumbFallback == nil {
                    thumbFallback = await pipeline.thumbnail(id: photoID)
                }
                image = await pipeline.preview(id: photoID, maxEdge: maxEdge)
            }
        }
    }

    private func fittedSize(for imageSize: CGSize?, in container: CGSize) -> CGSize {
        guard let imageSize, imageSize.width > 0, imageSize.height > 0,
              container.width > 0, container.height > 0 else {
            return container
        }
        let scaleFactor = min(container.width / imageSize.width, container.height / imageSize.height)
        return CGSize(width: imageSize.width * scaleFactor, height: imageSize.height * scaleFactor)
    }

    private func clampedOffset(_ raw: CGSize, fitted: CGSize, container: CGSize, scale: CGFloat) -> CGSize {
        let maxX = max(0, (fitted.width * scale - container.width) / 2)
        let maxY = max(0, (fitted.height * scale - container.height) / 2)
        return CGSize(
            width: min(maxX, max(-maxX, raw.width)),
            height: min(maxY, max(-maxY, raw.height))
        )
    }
}
