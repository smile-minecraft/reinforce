import SwiftUI

enum PracticalCanvasMode: Equatable {
    case locate
    case identify
    case view
}

struct PracticalCanvasView: View {
    let baseURL: URL?
    let overlayURL: URL?
    let pins: [DiagramPin]
    let mode: PracticalCanvasMode
    let placedClicks: [PinClick]
    let activePinNo: Int?
    let revealPins: Bool
    let l10n: LocalizationManager
    var onPlacePin: ((NormalizedPoint) -> Void)?

    @State private var scale: CGFloat = 1.0
    @State private var gestureScale: CGFloat = 1.0
    @State private var offset: CGSize = .zero
    @State private var dragTranslation: CGSize = .zero
    @State private var showOverlay: Bool = false

    private var effectiveScale: CGFloat {
        min(4.0, max(1.0, scale * gestureScale))
    }

    private var effectiveOffset: CGSize {
        if effectiveScale <= 1.01 { return .zero }
        return CGSize(
            width: offset.width + dragTranslation.width,
            height: offset.height + dragTranslation.height
        )
    }

    private var aspectRatio: CGFloat {
        MediaAssetCache.shared.asset(for: baseURL)?.aspectRatio ?? (16.0 / 10.0)
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider().background(ReinforceTheme.border)
            zoomStage
        }
        .background(ReinforceTheme.surface)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(ReinforceTheme.border, lineWidth: 1)
        )
    }

    private var toolbar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                Button {
                    adjustZoom(by: -0.5)
                } label: {
                    Image(systemName: "minus.magnifyingglass")
                        .frame(width: 36, height: 36)
                        .foregroundStyle(ReinforceTheme.ink)
                        .background(ReinforceTheme.surface)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(ReinforceTheme.border, lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .disabled(effectiveScale <= 1.01)
                .accessibilityLabel(l10n.t("study.runner.zoomOut"))
                .accessibilityIdentifier("practical-zoom-out")

                Text("\(Int(round(effectiveScale * 100)))%")
                    .font(.caption.monospacedDigit().weight(.semibold))
                    .foregroundStyle(ReinforceTheme.inkMuted)
                    .frame(minWidth: 44)
                    .accessibilityIdentifier("practical-zoom-percentage")

                Button {
                    adjustZoom(by: 0.5)
                } label: {
                    Image(systemName: "plus.magnifyingglass")
                        .frame(width: 36, height: 36)
                        .foregroundStyle(ReinforceTheme.ink)
                        .background(ReinforceTheme.surface)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(ReinforceTheme.border, lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .disabled(effectiveScale >= 3.99)
                .accessibilityLabel(l10n.t("study.runner.zoomIn"))
                .accessibilityIdentifier("practical-zoom-in")

                Button {
                    resetTransform()
                } label: {
                    Label(l10n.t("study.runner.zoomReset"), systemImage: "arrow.counterclockwise")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(ReinforceTheme.ink)
                        .padding(.horizontal, 10)
                        .frame(height: 36)
                        .background(ReinforceTheme.surface)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(ReinforceTheme.border, lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
                .disabled(effectiveScale <= 1.01 && offset == .zero)
                .accessibilityIdentifier("practical-zoom-reset")

                if overlayURL != nil && revealPins {
                    Button {
                        showOverlay.toggle()
                    } label: {
                        Label(
                            showOverlay ? l10n.t("blocks.pinsOverlayOn") : l10n.t("blocks.pinsOverlayOff"),
                            systemImage: "square.2.layers.3d"
                        )
                        .font(.caption.weight(.medium))
                        .foregroundStyle(showOverlay ? ReinforceTheme.accent : ReinforceTheme.ink)
                        .padding(.horizontal, 10)
                        .frame(height: 36)
                        .background(showOverlay ? ReinforceTheme.accentSoft : ReinforceTheme.surface)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(showOverlay ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                        )
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("practical-overlay-toggle")
                }

                Spacer(minLength: 8)

                Text(
                    mode == .locate && activePinNo != nil
                        ? l10n.t("study.runner.pinLocateTarget", activePinNo ?? 1)
                        : l10n.t("study.runner.zoomHint")
                )
                .font(.caption)
                .foregroundStyle(ReinforceTheme.inkMuted)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
        }
        .background(ReinforceTheme.subtle.opacity(0.65))
    }

    private var zoomStage: some View {
        GeometryReader { stageGeo in
            let fitted = fittedImageSize(in: stageGeo.size, aspectRatio: aspectRatio)
            ZStack {
                ReinforceTheme.subtle.opacity(0.4)

                ZStack(alignment: .topLeading) {
                    ResourceImageView(fileURL: baseURL, fallbackAspectRatio: aspectRatio)
                        .frame(width: fitted.width, height: fitted.height)

                    if let overlayURL, revealPins, showOverlay {
                        ResourceImageView(fileURL: overlayURL, fallbackAspectRatio: aspectRatio)
                            .frame(width: fitted.width, height: fitted.height)
                            .opacity(0.75)
                            .allowsHitTesting(false)
                    }

                    if mode == .identify || revealPins {
                        ForEach(pins) { pin in
                            VStack(spacing: 2) {
                                ZStack {
                                    Circle()
                                        .fill(revealPins ? ReinforceTheme.ok : ReinforceTheme.accent)
                                        .frame(width: 28, height: 28)
                                        .overlay(Circle().stroke(.white, lineWidth: 2))
                                        .shadow(color: .black.opacity(0.25), radius: 3, y: 1)
                                    Text("\(pin.no)")
                                        .font(.caption.monospacedDigit().weight(.bold))
                                        .foregroundStyle(.white)
                                }
                                if revealPins {
                                    Text(pin.termPrimary)
                                        .font(.caption2.weight(.semibold))
                                        .foregroundStyle(ReinforceTheme.ink)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 2)
                                        .background(ReinforceTheme.surface.opacity(0.95))
                                        .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                                        .shadow(radius: 2)
                                }
                            }
                            .position(
                                x: CGFloat(pin.x) * fitted.width,
                                y: CGFloat(pin.y) * fitted.height
                            )
                            .allowsHitTesting(false)
                        }
                    }

                    ForEach(placedClicks, id: \.no) { click in
                        ZStack {
                            Circle()
                                .fill(ReinforceTheme.warn)
                                .frame(width: 30, height: 30)
                                .overlay(Circle().stroke(.white, lineWidth: 2))
                                .shadow(color: .black.opacity(0.28), radius: 3, y: 1)
                            Text("\(click.no)")
                                .font(.caption.monospacedDigit().weight(.bold))
                                .foregroundStyle(.white)
                        }
                        .position(
                            x: CGFloat(click.point.x) * fitted.width,
                            y: CGFloat(click.point.y) * fitted.height
                        )
                        .allowsHitTesting(false)
                        .accessibilityIdentifier("placed-pin-\(click.no)")
                    }
                }
                .frame(width: fitted.width, height: fitted.height)
                .contentShape(Rectangle())
                .scaleEffect(effectiveScale)
                .offset(effectiveOffset)
                .gesture(
                    SpatialTapGesture()
                        .onEnded { value in
                            guard mode == .locate, let onPlacePin else { return }
                            let nx = value.location.x / max(1, fitted.width)
                            let ny = value.location.y / max(1, fitted.height)
                            guard nx >= 0, nx <= 1, ny >= 0, ny <= 1 else { return }
                            onPlacePin(
                                NormalizedPoint(
                                    x: (Double(nx) * 10000).rounded() / 10000,
                                    y: (Double(ny) * 10000).rounded() / 10000
                                )
                            )
                        }
                )
            }
            .frame(width: stageGeo.size.width, height: stageGeo.size.height)
            .clipped()
            .contentShape(Rectangle())
            .simultaneousGesture(
                MagnifyGesture()
                    .onChanged { value in
                        gestureScale = value.magnification
                    }
                    .onEnded { value in
                        scale = min(4.0, max(1.0, scale * value.magnification))
                        gestureScale = 1.0
                        if scale <= 1.01 {
                            scale = 1.0
                            offset = .zero
                        }
                    }
            )
            .simultaneousGesture(
                DragGesture(minimumDistance: 10)
                    .onChanged { value in
                        guard effectiveScale > 1.01 else { return }
                        dragTranslation = value.translation
                    }
                    .onEnded { value in
                        guard effectiveScale > 1.01 else {
                            dragTranslation = .zero
                            return
                        }
                        offset = CGSize(
                            width: offset.width + value.translation.width,
                            height: offset.height + value.translation.height
                        )
                        dragTranslation = .zero
                    }
            )
        }
        .frame(minHeight: 280, maxHeight: 420)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("practical-canvas")
    }

    private func fittedImageSize(in container: CGSize, aspectRatio: CGFloat) -> CGSize {
        guard container.width > 0, container.height > 0, aspectRatio > 0 else {
            return container
        }
        let containerRatio = container.width / container.height
        if containerRatio > aspectRatio {
            let height = container.height
            return CGSize(width: height * aspectRatio, height: height)
        } else {
            let width = container.width
            return CGSize(width: width, height: width / aspectRatio)
        }
    }

    private func adjustZoom(by delta: CGFloat) {
        let next = min(4.0, max(1.0, (scale + delta * 100 / 100)))
        scale = (next * 100).rounded() / 100
        if scale <= 1.01 {
            scale = 1.0
            offset = .zero
        }
    }

    private func resetTransform() {
        scale = 1.0
        gestureScale = 1.0
        offset = .zero
        dragTranslation = .zero
    }
}
