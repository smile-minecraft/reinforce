import { ExternalLink, Layers, Tag } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { resourceBlobUrl } from '../api/client'
import type { Pin } from '../api/types'

export function InteractivePinsBlock({
  resourceId,
  overlayResourceId,
  pins,
  hideLabelsAndTerms = false,
}: {
  resourceId: string
  overlayResourceId: string | null
  pins: Pin[]
  hideLabelsAndTerms?: boolean
}) {
  const { t } = useTranslation()
  const [showOverlay, setShowOverlay] = useState(false)
  const [showLabels, setShowLabels] = useState(!hideLabelsAndTerms)
  const [selectedNo, setSelectedNo] = useState<number | null>(
    !hideLabelsAndTerms && pins.length > 0 ? pins[0].no : null,
  )

  const selectedPin = pins.find((p) => p.no === selectedNo) ?? null

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-border bg-surface">
      {!hideLabelsAndTerms && (
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-subtle/60 px-3 py-2">
          <div className="flex flex-wrap items-center gap-2">
            {overlayResourceId && (
              <button
                type="button"
                onClick={() => setShowOverlay((v) => !v)}
                aria-pressed={showOverlay}
                className={`inline-flex min-h-11 items-center gap-1.5 rounded-lg border px-3 text-xs font-medium transition-colors ${
                  showOverlay
                    ? 'border-accent bg-accent-soft text-accent'
                    : 'border-border bg-surface text-ink hover:bg-subtle'
                }`}
              >
                <Layers className="h-3.5 w-3.5" />
                <span>{showOverlay ? t('blocks.pinsOverlayOn') : t('blocks.pinsOverlayOff')}</span>
              </button>
            )}

            <button
              type="button"
              onClick={() => setShowLabels((v) => !v)}
              aria-pressed={showLabels}
              className={`inline-flex min-h-11 items-center gap-1.5 rounded-lg border px-3 text-xs font-medium transition-colors ${
                showLabels
                  ? 'border-accent bg-accent-soft text-accent'
                  : 'border-border bg-surface text-ink hover:bg-subtle'
              }`}
            >
              <Tag className="h-3.5 w-3.5" />
              <span>{showLabels ? t('blocks.pinsLabelsOn') : t('blocks.pinsLabelsOff')}</span>
            </button>
          </div>
          <span className="text-xs text-ink-muted">{t('blocks.pinTapPrompt')}</span>
        </div>
      )}

      <div className="relative select-none overflow-hidden bg-subtle/30">
        <img
          src={resourceBlobUrl(resourceId)}
          alt=""
          className="block h-auto w-full"
          draggable={false}
        />

        {overlayResourceId && showOverlay && !hideLabelsAndTerms && (
          <img
            src={resourceBlobUrl(overlayResourceId)}
            alt=""
            className="pointer-events-none absolute inset-0 h-full w-full object-fill opacity-70"
            draggable={false}
          />
        )}

        {/* Leader lines connecting pin coordinates to label coordinates */}
        {showLabels && !hideLabelsAndTerms && (
          <svg
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
            className="pointer-events-none absolute inset-0 h-full w-full"
          >
            {pins.map((pin) => (
              <line
                key={`line-${pin.no}`}
                x1={pin.x * 100}
                y1={pin.y * 100}
                x2={pin.label_x * 100}
                y2={pin.label_y * 100}
                stroke={selectedNo === pin.no ? 'var(--rf-accent)' : 'currentColor'}
                strokeOpacity={selectedNo === pin.no ? 0.9 : 0.45}
                strokeWidth="0.4"
                strokeDasharray="1.2 0.8"
              />
            ))}
          </svg>
        )}

        {/* Label tags */}
        {showLabels &&
          !hideLabelsAndTerms &&
          pins.map((pin) => {
            const active = selectedNo === pin.no
            return (
              <button
                key={`label-${pin.no}`}
                type="button"
                onClick={() => setSelectedNo(pin.no)}
                style={{
                  left: `${pin.label_x * 100}%`,
                  top: `${pin.label_y * 100}%`,
                }}
                className={`absolute -translate-x-1/2 -translate-y-1/2 rounded-md border px-2 py-0.5 text-xs font-medium shadow-sm transition-colors ${
                  active
                    ? 'z-20 border-accent bg-accent text-white'
                    : 'z-10 border-border bg-surface/95 text-ink hover:border-accent'
                }`}
              >
                <span className="mr-1 font-mono opacity-80">{pin.no}</span>
                {pin.term_primary}
              </button>
            )
          })}

        {/* Numbered pin buttons (44px touch area centered on pin point) */}
        {pins.map((pin) => {
          const active = selectedNo === pin.no
          return (
            <button
              key={`pin-${pin.no}`}
              type="button"
              onClick={() => !hideLabelsAndTerms && setSelectedNo(pin.no)}
              style={{
                left: `${pin.x * 100}%`,
                top: `${pin.y * 100}%`,
              }}
              aria-label={t('blocks.pinSelected', { no: pin.no })}
              aria-pressed={active}
              className="group absolute z-20 flex h-11 w-11 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full"
            >
              <span
                className={`flex h-7 w-7 items-center justify-center rounded-full border-2 font-mono text-xs font-bold shadow transition-transform ${
                  active
                    ? 'scale-110 border-white bg-accent text-white ring-2 ring-accent/40'
                    : 'border-white bg-ink text-canvas group-hover:scale-105'
                }`}
              >
                {pin.no}
              </span>
            </button>
          )
        })}
      </div>

      {/* Selected pin detail bar */}
      {!hideLabelsAndTerms && selectedPin && (
        <div className="border-t border-border bg-surface p-4">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <div className="flex flex-wrap items-baseline gap-2">
              <span className="inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent font-mono text-xs font-bold text-white">
                {selectedPin.no}
              </span>
              <span className="text-base font-semibold text-ink">{selectedPin.term_primary}</span>
              {selectedPin.term_secondary && (
                <span className="text-sm text-ink-muted">{selectedPin.term_secondary}</span>
              )}
            </div>

            {selectedPin.linked_card_id && (
              <Link
                to={`/cards/${encodeURIComponent(selectedPin.linked_card_id)}`}
                className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border px-3 text-xs font-medium text-accent hover:bg-accent-soft"
              >
                <span>{t('blocks.pinLinkedCard')}</span>
                <ExternalLink className="h-3.5 w-3.5" />
              </Link>
            )}
          </div>

          {selectedPin.detail && (
            <p className="mt-2 text-sm text-ink">
              <span className="mr-1.5 font-medium text-ink-muted">{t('blocks.pinDetail')}:</span>
              {selectedPin.detail}
            </p>
          )}

          {selectedPin.hint && (
            <p className="mt-1.5 rounded-lg bg-subtle px-3 py-2 text-xs text-ink-muted">
              <span className="mr-1.5 font-semibold text-ink">{t('blocks.pinHint')}:</span>
              {selectedPin.hint}
            </p>
          )}
        </div>
      )}
    </div>
  )
}
