import { AlertCircle, Check, GripHorizontal, Layers, Loader2, Tag } from 'lucide-react'
import { useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { extractValidationReport, resourceBlobUrl } from '../api/client'
import { useAdjustProposalPins } from '../api/hooks/useProposals'
import type { Pin, ValidationReport } from '../api/types'

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value))
}

function roundCoord(value: number): number {
  return Math.round(clamp01(value) * 10000) / 10000
}

interface DragSession {
  pinNo: number
  target: 'tip' | 'label'
  pointerId: number
  moved: boolean
  snapshot: Pin[]
}

export function PinCalibrationBlock({
  proposalId,
  resourceId,
  overlayResourceId,
  pins,
  readOnly = false,
  onValidationError,
}: {
  proposalId: string
  resourceId: string
  overlayResourceId: string | null
  pins: Pin[]
  readOnly?: boolean
  onValidationError?: (report: ValidationReport) => void
}) {
  const { t } = useTranslation()
  const adjustPins = useAdjustProposalPins()
  const imgRef = useRef<HTMLImageElement | null>(null)
  const dragRef = useRef<DragSession | null>(null)

  const [overridePins, setOverridePins] = useState<Pin[] | null>(null)
  const [draggingPinNo, setDraggingPinNo] = useState<number | null>(null)
  const [selectedNo, setSelectedNo] = useState<number | null>(pins.length > 0 ? pins[0].no : null)
  const [showOverlay, setShowOverlay] = useState(false)
  const [showLabels, setShowLabels] = useState(true)
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle')
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const activePins = overridePins ?? pins
  const selectedPin = activePins.find((p) => p.no === selectedNo) ?? activePins[0] ?? null

  function computeNormalizedPoint(clientX: number, clientY: number): { x: number; y: number } | null {
    const img = imgRef.current
    if (!img) return null
    const rect = img.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return null
    return {
      x: roundCoord((clientX - rect.left) / rect.width),
      y: roundCoord((clientY - rect.top) / rect.height),
    }
  }

  function handleHandlePointerDown(
    e: ReactPointerEvent<HTMLButtonElement>,
    pinNo: number,
    target: 'tip' | 'label',
  ) {
    e.stopPropagation()
    setSelectedNo(pinNo)
    if (readOnly) return

    e.currentTarget.setPointerCapture(e.pointerId)
    const snapshot = activePins.map((p) => ({ ...p }))
    dragRef.current = {
      pinNo,
      target,
      pointerId: e.pointerId,
      moved: false,
      snapshot,
    }
    setDraggingPinNo(pinNo)
    setErrorMessage(null)
    setSaveState('idle')
  }

  function handleHandlePointerMove(e: ReactPointerEvent<HTMLButtonElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    e.stopPropagation()

    const pt = computeNormalizedPoint(e.clientX, e.clientY)
    if (!pt) return
    drag.moved = true

    setOverridePins((prev) => {
      const base = prev ?? drag.snapshot
      return base.map((pin) => {
        if (pin.no !== drag.pinNo) return pin
        if (drag.target === 'tip') {
          return { ...pin, x: pt.x, y: pt.y }
        }
        return { ...pin, label_x: pt.x, label_y: pt.y }
      })
    })
  }

  async function commitPinUpdate(updatedPin: Pin, snapshot: Pin[]) {
    setSaveState('saving')
    setErrorMessage(null)
    try {
      await adjustPins.mutateAsync({
        id: proposalId,
        body: {
          resource_id: resourceId,
          pins: [
            {
              no: updatedPin.no,
              x: updatedPin.x,
              y: updatedPin.y,
              label_x: updatedPin.label_x,
              label_y: updatedPin.label_y,
            },
          ],
        },
      })
      setSaveState('saved')
      setOverridePins(null)
    } catch (err) {
      setOverridePins(snapshot)
      setSaveState('error')
      const report = extractValidationReport(err)
      if (report && onValidationError) {
        onValidationError(report)
      }
      setErrorMessage(err instanceof Error ? err.message : t('triage.pins.saveError'))
    }
  }

  function handleHandlePointerUp(e: ReactPointerEvent<HTMLButtonElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    e.stopPropagation()
    dragRef.current = null
    setDraggingPinNo(null)

    if (!drag.moved) return

    const pt = computeNormalizedPoint(e.clientX, e.clientY)
    const base = overridePins ?? drag.snapshot
    const updatedList = pt
      ? base.map((pin) => {
          if (pin.no !== drag.pinNo) return pin
          return drag.target === 'tip'
            ? { ...pin, x: pt.x, y: pt.y }
            : { ...pin, label_x: pt.x, label_y: pt.y }
        })
      : base
    setOverridePins(updatedList)

    const updatedPin = updatedList.find((p) => p.no === drag.pinNo)
    if (updatedPin) {
      void commitPinUpdate(updatedPin, drag.snapshot)
    }
  }

  function handleHandlePointerCancel(e: ReactPointerEvent<HTMLButtonElement>) {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    e.stopPropagation()
    dragRef.current = null
    setDraggingPinNo(null)
    setOverridePins(drag.snapshot)
  }

  return (
    <div
      data-no-swipe="true"
      className="my-3 overflow-hidden rounded-xl border border-border bg-surface"
    >
      {/* Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-subtle/60 px-3 py-2">
        <div className="flex flex-wrap items-center gap-2">
          {overlayResourceId && (
            <button
              type="button"
              onClick={() => setShowOverlay((v) => !v)}
              aria-pressed={showOverlay}
              className={`inline-flex min-h-10 items-center gap-1.5 rounded-lg border px-2.5 text-xs font-medium transition-colors ${
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
            className={`inline-flex min-h-10 items-center gap-1.5 rounded-lg border px-2.5 text-xs font-medium transition-colors ${
              showLabels
                ? 'border-accent bg-accent-soft text-accent'
                : 'border-border bg-surface text-ink hover:bg-subtle'
            }`}
          >
            <Tag className="h-3.5 w-3.5" />
            <span>{showLabels ? t('blocks.pinsLabelsOn') : t('blocks.pinsLabelsOff')}</span>
          </button>
        </div>

        <div className="flex items-center gap-2 text-xs">
          {saveState === 'saving' && (
            <span className="inline-flex items-center gap-1 font-medium text-accent">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t('triage.pins.saving')}
            </span>
          )}
          {saveState === 'saved' && (
            <span className="inline-flex items-center gap-1 font-medium text-ok">
              <Check className="h-3.5 w-3.5" />
              {t('triage.pins.saved')}
            </span>
          )}
          {saveState === 'idle' && (
            <span className="text-ink-muted">
              {readOnly ? t('triage.pins.readOnly') : t('triage.pins.calibrateHint')}
            </span>
          )}
        </div>
      </div>

      {saveState === 'error' && errorMessage && (
        <div
          role="alert"
          className="flex items-center gap-2 border-b border-bad/30 bg-bad-soft px-3 py-2 text-xs text-bad"
        >
          <AlertCircle className="h-4 w-4 shrink-0" />
          <span>
            {t('triage.pins.saveError')} ({errorMessage})
          </span>
        </div>
      )}

      {/* Draggable Pin Stage */}
      <div className="relative select-none overflow-hidden bg-subtle/30">
        <img
          ref={imgRef}
          src={resourceBlobUrl(resourceId)}
          alt=""
          className="block h-auto w-full"
          draggable={false}
        />

        {overlayResourceId && showOverlay && (
          <img
            src={resourceBlobUrl(overlayResourceId)}
            alt=""
            className="pointer-events-none absolute inset-0 h-full w-full object-fill opacity-70"
            draggable={false}
          />
        )}

        {/* Dynamic leader lines */}
        {showLabels && (
          <svg
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
            className="pointer-events-none absolute inset-0 h-full w-full"
          >
            {activePins.map((pin) => {
              const active = selectedPin?.no === pin.no || draggingPinNo === pin.no
              return (
                <line
                  key={`line-${pin.no}`}
                  x1={pin.x * 100}
                  y1={pin.y * 100}
                  x2={pin.label_x * 100}
                  y2={pin.label_y * 100}
                  stroke={active ? 'var(--rf-accent)' : 'currentColor'}
                  strokeOpacity={active ? 0.95 : 0.5}
                  strokeWidth={active ? '0.6' : '0.4'}
                  strokeDasharray="1.2 0.8"
                />
              )
            })}
          </svg>
        )}

        {/* Draggable Label handles */}
        {showLabels &&
          activePins.map((pin) => {
            const active = selectedPin?.no === pin.no
            const dragging = draggingPinNo === pin.no
            return (
              <button
                key={`label-${pin.no}`}
                type="button"
                onPointerDown={(e) => handleHandlePointerDown(e, pin.no, 'label')}
                onPointerMove={handleHandlePointerMove}
                onPointerUp={handleHandlePointerUp}
                onPointerCancel={handleHandlePointerCancel}
                style={{
                  left: `${pin.label_x * 100}%`,
                  top: `${pin.label_y * 100}%`,
                  touchAction: readOnly ? 'auto' : 'none',
                }}
                className={`absolute flex min-h-8 -translate-x-1/2 -translate-y-1/2 items-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium shadow-sm ${
                  readOnly ? 'cursor-pointer' : 'cursor-grab active:cursor-grabbing'
                } ${
                  dragging
                    ? 'z-30 scale-105 border-accent bg-accent text-white ring-2 ring-accent/40'
                    : active
                      ? 'z-20 border-accent bg-accent text-white'
                      : 'z-10 border-border bg-surface/95 text-ink hover:border-accent'
                }`}
              >
                {!readOnly && <GripHorizontal className="h-3 w-3 opacity-70" />}
                <span className="font-mono opacity-80">{pin.no}</span>
                <span className="max-w-32 truncate">{pin.term_primary}</span>
              </button>
            )
          })}

        {/* Draggable Pin Tip handles (44px touch target) */}
        {activePins.map((pin) => {
          const active = selectedPin?.no === pin.no
          const dragging = draggingPinNo === pin.no
          return (
            <button
              key={`pin-${pin.no}`}
              type="button"
              onPointerDown={(e) => handleHandlePointerDown(e, pin.no, 'tip')}
              onPointerMove={handleHandlePointerMove}
              onPointerUp={handleHandlePointerUp}
              onPointerCancel={handleHandlePointerCancel}
              style={{
                left: `${pin.x * 100}%`,
                top: `${pin.y * 100}%`,
                touchAction: readOnly ? 'auto' : 'none',
              }}
              aria-label={t('blocks.pinSelected', { no: pin.no })}
              aria-pressed={active}
              className={`group absolute flex h-11 w-11 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full ${
                dragging ? 'z-30' : 'z-20'
              } ${readOnly ? 'cursor-pointer' : 'cursor-grab active:cursor-grabbing'}`}
            >
              <span
                className={`flex h-7 w-7 items-center justify-center rounded-full border-2 font-mono text-xs font-bold shadow transition-transform ${
                  dragging
                    ? 'scale-125 border-white bg-accent text-white ring-4 ring-accent/40'
                    : active
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

      {/* Selected Pin Coordinate Readout */}
      {selectedPin && (
        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border bg-surface px-4 py-3 text-xs">
          <div className="flex flex-wrap items-baseline gap-2">
            <span className="inline-flex h-5 w-5 items-center justify-center rounded-full bg-accent font-mono text-[11px] font-bold text-white">
              {selectedPin.no}
            </span>
            <span className="font-semibold text-ink">{selectedPin.term_primary}</span>
            {selectedPin.term_secondary && (
              <span className="text-ink-muted">{selectedPin.term_secondary}</span>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2 font-mono text-[11px] text-ink-muted">
            <span className="rounded bg-subtle px-2 py-0.5">
              {t('triage.pins.tipCoords', {
                x: selectedPin.x.toFixed(3),
                y: selectedPin.y.toFixed(3),
              })}
            </span>
            <span className="rounded bg-subtle px-2 py-0.5">
              {t('triage.pins.labelCoords', {
                x: selectedPin.label_x.toFixed(3),
                y: selectedPin.label_y.toFixed(3),
              })}
            </span>
          </div>
        </div>
      )}
    </div>
  )
}
