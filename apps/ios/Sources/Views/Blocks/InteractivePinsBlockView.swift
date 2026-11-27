import SwiftUI

struct InteractivePinsBlockView: View {
    let resourceId: String
    let overlayResourceId: String?
    let pins: [DiagramPin]
    let baseURL: URL?
    let overlayURL: URL?
    let l10n: LocalizationManager
    var hideLabelsAndTerms: Bool = false
    var onOpenCard: ((String) -> Void)?

    @State private var showOverlay: Bool = false
    @State private var showLabels: Bool = true
    @State private var selectedPinNo: Int?

    private var aspectRatio: CGFloat {
        MediaAssetCache.shared.asset(for: baseURL)?.aspectRatio ?? (16.0 / 10.0)
    }

    private var selectedPin: DiagramPin? {
        guard !hideLabelsAndTerms else { return nil }
        if let selectedPinNo, let matched = pins.first(where: { $0.no == selectedPinNo }) {
            return matched
        }
        return pins.first
    }

    var body: some View {
        VStack(spacing: 0) {
            if !hideLabelsAndTerms {
                HStack(spacing: 8) {
                    if overlayURL != nil {
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
                            .frame(minHeight: 36)
                            .background(showOverlay ? ReinforceTheme.accentSoft : ReinforceTheme.surface)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .stroke(showOverlay ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("pins-overlay-toggle")
                    }

                    Button {
                        showLabels.toggle()
                    } label: {
                        Label(
                            showLabels ? l10n.t("blocks.pinsLabelsOn") : l10n.t("blocks.pinsLabelsOff"),
                            systemImage: "tag"
                        )
                        .font(.caption.weight(.medium))
                        .foregroundStyle(showLabels ? ReinforceTheme.accent : ReinforceTheme.ink)
                        .padding(.horizontal, 10)
                        .frame(minHeight: 36)
                        .background(showLabels ? ReinforceTheme.accentSoft : ReinforceTheme.surface)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .stroke(showLabels ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                        )
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("pins-labels-toggle")

                    Spacer(minLength: 4)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(ReinforceTheme.subtle.opacity(0.65))

                Divider().background(ReinforceTheme.border)
            }

            GeometryReader { geo in
                ZStack(alignment: .topLeading) {
                    ResourceImageView(
                        fileURL: baseURL,
                        fallbackAspectRatio: aspectRatio,
                        accessibilityId: "interactive-pins-base-image"
                    )
                    .frame(width: geo.size.width, height: geo.size.height)

                    if let overlayURL, showOverlay, !hideLabelsAndTerms {
                        ResourceImageView(
                            fileURL: overlayURL,
                            fallbackAspectRatio: aspectRatio,
                            accessibilityId: "interactive-pins-overlay-image"
                        )
                        .frame(width: geo.size.width, height: geo.size.height)
                        .opacity(0.75)
                        .allowsHitTesting(false)
                    }

                    if showLabels && !hideLabelsAndTerms {
                        Canvas { context, size in
                            for pin in pins {
                                let isActive = selectedPin?.no == pin.no
                                var path = Path()
                                path.move(to: CGPoint(x: pin.x * size.width, y: pin.y * size.height))
                                path.addLine(to: CGPoint(x: pin.labelX * size.width, y: pin.labelY * size.height))
                                context.stroke(
                                    path,
                                    with: .color(isActive ? ReinforceTheme.accent : ReinforceTheme.inkMuted.opacity(0.6)),
                                    style: StrokeStyle(lineWidth: isActive ? 2 : 1.25, dash: [4, 3])
                                )
                            }
                        }
                        .allowsHitTesting(false)

                        ForEach(pins) { pin in
                            let isActive = selectedPin?.no == pin.no
                            Button {
                                selectedPinNo = pin.no
                            } label: {
                                HStack(spacing: 4) {
                                    Text("\(pin.no)")
                                        .font(.caption2.monospacedDigit().weight(.bold))
                                        .opacity(0.85)
                                    Text(pin.termPrimary)
                                        .font(.caption2.weight(.semibold))
                                        .lineLimit(1)
                                }
                                .padding(.horizontal, 7)
                                .padding(.vertical, 3)
                                .foregroundStyle(isActive ? .white : ReinforceTheme.ink)
                                .background(isActive ? ReinforceTheme.accent : ReinforceTheme.surface.opacity(0.95))
                                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                        .stroke(isActive ? ReinforceTheme.accent : ReinforceTheme.border, lineWidth: 1)
                                )
                                .shadow(color: .black.opacity(0.12), radius: 2, y: 1)
                            }
                            .buttonStyle(.plain)
                            .position(x: pin.labelX * geo.size.width, y: pin.labelY * geo.size.height)
                        }
                    }

                    ForEach(pins) { pin in
                        let isActive = !hideLabelsAndTerms && selectedPin?.no == pin.no
                        Button {
                            if !hideLabelsAndTerms {
                                selectedPinNo = pin.no
                            }
                        } label: {
                            ZStack {
                                Circle()
                                    .fill(isActive ? ReinforceTheme.accent : ReinforceTheme.ink)
                                    .frame(width: 28, height: 28)
                                    .overlay(Circle().stroke(.white, lineWidth: 2))
                                    .shadow(color: .black.opacity(0.22), radius: 3, y: 1)
                                Text("\(pin.no)")
                                    .font(.caption.monospacedDigit().weight(.bold))
                                    .foregroundStyle(isActive ? .white : ReinforceTheme.canvas)
                            }
                            .frame(width: ReinforceTheme.minTouchTarget, height: ReinforceTheme.minTouchTarget)
                        }
                        .buttonStyle(.plain)
                        .position(x: pin.x * geo.size.width, y: pin.y * geo.size.height)
                        .accessibilityLabel(l10n.t("blocks.pinSelected", pin.no))
                        .accessibilityIdentifier("interactive-pin-\(pin.no)")
                    }
                }
            }
            .aspectRatio(aspectRatio, contentMode: .fit)
            .background(ReinforceTheme.subtle.opacity(0.3))

            if let pin = selectedPin {
                Divider().background(ReinforceTheme.border)
                VStack(alignment: .leading, spacing: 8) {
                    HStack(alignment: .center, spacing: 8) {
                        Text("\(pin.no)")
                            .font(.caption.monospacedDigit().weight(.bold))
                            .foregroundStyle(.white)
                            .frame(width: 24, height: 24)
                            .background(ReinforceTheme.accent)
                            .clipShape(Circle())

                        Text(pin.termPrimary)
                            .font(.headline)
                            .foregroundStyle(ReinforceTheme.ink)

                        if let secondary = pin.termSecondary, !secondary.isEmpty {
                            Text(secondary)
                                .font(.subheadline)
                                .foregroundStyle(ReinforceTheme.inkMuted)
                        }

                        Spacer()

                        if let linkedId = pin.linkedCardId, let onOpenCard {
                            Button {
                                onOpenCard(linkedId)
                            } label: {
                                Label(l10n.t("blocks.pinLinkedCard"), systemImage: "arrow.up.right")
                                    .font(.caption.weight(.medium))
                                    .foregroundStyle(ReinforceTheme.accent)
                                    .padding(.horizontal, 10)
                                    .frame(minHeight: 34)
                                    .background(ReinforceTheme.accentSoft)
                                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                            }
                            .buttonStyle(.plain)
                        }
                    }

                    if let detail = pin.detail, !detail.isEmpty {
                        Text("\(l10n.t("blocks.pinDetail")): \(detail)")
                            .font(.subheadline)
                            .foregroundStyle(ReinforceTheme.ink)
                    }

                    if let hint = pin.hint, !hint.isEmpty {
                        Text("\(l10n.t("blocks.pinHint")): \(hint)")
                            .font(.caption)
                            .foregroundStyle(ReinforceTheme.inkMuted)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(ReinforceTheme.subtle)
                            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                    }
                }
                .padding(12)
                .background(ReinforceTheme.surface)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(ReinforceTheme.border, lineWidth: 1)
        )
        .onAppear {
            if selectedPinNo == nil && !hideLabelsAndTerms {
                selectedPinNo = pins.first?.no
            }
        }
    }
}
