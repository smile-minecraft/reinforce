import { Layers, RotateCcw, ZoomIn, ZoomOut } from 'lucide-react'
import { useRef, useState, type PointerEvent as ReactPointerEvent, type WheelEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { resourceBlobUrl } from '../api/client'
import type { NormalizedPoint, Pin, PinClick } from '../api/types'

interface PointerPos {
  x: number
  y: number
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v))
}

function distance(a: PointerPos, b: PointerPos): number {
  return Math.hypot(a.x - b.x, a.y - b.y)
}

export function PracticalCanvas({
  resourceId,
  overlayResourceId,
  pins,
  mode,
  placedClicks,
  activePinNo,
  revealPins = false,
  onPlacePin,
}: {
  resourceId: string
  overlayResourceId?: string | null
  pins: Pin[]
  mode: 'locate' | 'identify' | 'view'
  placedClicks?: PinClick[]
  activePinNo?: number | null
  revealPins?: boolean
  onPlacePin?: (point: NormalizedPoint) => void
}) {
  const { t } = useTranslation()
  const imgRef = useRef<HTMLImageElement | null>(null)
  const pointersRef = useRef<Map<number, PointerPos>>(new Map())
  const pinchStartRef = useRef<{ dist: number; scale: number } | null>(null)
  const dragStartRef = useRef<{
    pointerId: number
    startX: number
    startY: number
    originTx: number
    originTy: number
    moved: boolean
  } | null>(null)

  const [scale, setScale] = useState(1)
  const [tx, setTx] = useState(0)
  const [ty, setTy] = useState(0)
  const [showOverlay, setShowOverlay] = useState(false)

  function resetTransform() {
    setScale(1)
    setTx(0)
    setTy(0)
  }

  function zoomBy(delta: number) {
    setScale((prev) => {
      const next = clamp(Number((prev + delta).toFixed(2)), 1, 4)
      if (next === 1) {
        setTx(0)
        setTy(0)
      }
      return next
    })
  }

  function handlePointerDown(e: ReactPointerEvent<HTMLDivElement>) {
    e.currentTarget.setPointerCapture(e.pointerId)
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY })

    if (pointersRef.current.size === 2) {
      const [p1, p2] = Array.from(pointersRef.current.values())
      pinchStartRef.current = { dist: Math.max(1, distance(p1, p2)), scale }
      dragStartRef.current = null
    } else if (pointersRef.current.size === 1) {
      dragStartRef.current = {
        pointerId: e.pointerId,
        startX: e.clientX,
        startY: e.clientY,
        originTx: tx,
        originTy: ty,
        moved: false,
      }
    }
  }

  function handlePointerMove(e: ReactPointerEvent<HTMLDivElement>) {
    if (!pointersRef.current.has(e.pointerId)) return
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY })

    if (pointersRef.current.size === 2 && pinchStartRef.current) {
      const [p1, p2] = Array.from(pointersRef.current.values())
      const nextScale = clamp(
        pinchStartRef.current.scale * (distance(p1, p2) / pinchStartRef.current.dist),
        1,
        4,
      )
      setScale(nextScale)
      if (nextScale <= 1.01) {
        setTx(0)
        setTy(0)
      }
      return
    }

    const drag = dragStartRef.current
    if (drag && drag.pointerId === e.pointerId) {
      const dx = e.clientX - drag.startX
      const dy = e.clientY - drag.startY
      if (Math.hypot(dx, dy) > 8) {
        drag.moved = true
      }
      if (scale > 1) {
        setTx(drag.originTx + dx)
        setTy(drag.originTy + dy)
      }
    }
  }

  function handlePointerUp(e: ReactPointerEvent<HTMLDivElement>) {
    const drag = dragStartRef.current
    const wasSingleTap =
      pointersRef.current.size === 1 && drag && drag.pointerId === e.pointerId && !drag.moved

    pointersRef.current.delete(e.pointerId)
    if (pointersRef.current.size < 2) {
      pinchStartRef.current = null
    }
    if (drag?.pointerId === e.pointerId) {
      dragStartRef.current = null
    }

    if (wasSingleTap && mode === 'locate' && onPlacePin && imgRef.current) {
      const rect = imgRef.current.getBoundingClientRect()
      if (rect.width > 0 && rect.height > 0) {
        const nx = (e.clientX - rect.left) / rect.width
        const ny = (e.clientY - rect.top) / rect.height
        if (nx >= 0 && nx <= 1 && ny >= 0 && ny <= 1) {
          onPlacePin({
            x: Number(clamp(nx, 0, 1).toFixed(4)),
            y: Number(clamp(ny, 0, 1).toFixed(4)),
          })
        }
      }
    }
  }

  function handleWheel(e: WheelEvent<HTMLDivElement>) {
    if (!e.ctrlKey && !e.metaKey) return
    e.preventDefault()
    zoomBy(e.deltaY < 0 ? 0.35 : -0.35)
  }

  const showAnswerOverlay = Boolean(overlayResourceId && revealPins && showOverlay)

  return (
    <div className="overflow-hidden rounded-xl border border-border bg-surface">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-subtle/60 px-3 py-1.5">
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => zoomBy(-0.5)}
            disabled={scale <= 1}
            aria-label={t('study.runner.zoomOut')}
            className="flex min-h-11 min-w-11 items-center justify-center rounded-lg border border-border bg-surface text-ink hover:bg-subtle disabled:opacity-40"
          >
            <ZoomOut className="h-4 w-4" />
          </button>
          <span className="min-w-12 text-center font-mono text-xs text-ink-muted">
            {Math.round(scale * 100)}%
          </span>
          <button
            type="button"
            onClick={() => zoomBy(0.5)}
            disabled={scale >= 4}
            aria-label={t('study.runner.zoomIn')}
            className="flex min-h-11 min-w-11 items-center justify-center rounded-lg border border-border bg-surface text-ink hover:bg-subtle disabled:opacity-40"
          >
            <ZoomIn className="h-4 w-4" />
          </button>
          <button
            type="button"
            onClick={resetTransform}
            disabled={scale === 1 && tx === 0 && ty === 0}
            className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-surface px-3 text-xs font-medium text-ink hover:bg-subtle disabled:opacity-40"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            <span>{t('study.runner.zoomReset')}</span>
          </button>

          {overlayResourceId && revealPins && (
            <button
              type="button"
              onClick={() => setShowOverlay((v) => !v)}
              aria-pressed={showOverlay}
              className={`inline-flex min-h-11 items-center gap-1.5 rounded-lg border px-3 text-xs font-medium ${
                showOverlay
                  ? 'border-accent bg-accent-soft text-accent'
                  : 'border-border bg-surface text-ink hover:bg-subtle'
              }`}
            >
              <Layers className="h-3.5 w-3.5" />
              <span>{showOverlay ? t('blocks.pinsOverlayOn') : t('blocks.pinsOverlayOff')}</span>
            </button>
          )}
        </div>

        <span className="text-xs text-ink-muted">
          {mode === 'locate' && activePinNo != null
            ? t('study.runner.pinLocateTarget', { no: activePinNo })
            : t('study.runner.zoomHint')}
        </span>
      </div>

      {/* Zoomable Stage: 55svh portrait, 65svh landscape */}
      <div
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onWheel={handleWheel}
        style={{ touchAction: 'none' }}
        className={`relative flex h-[55svh] landscape:h-[65svh] w-full items-center justify-center overflow-hidden bg-subtle/40 select-none ${
          mode === 'locate' ? 'cursor-crosshair' : scale > 1 ? 'cursor-grab active:cursor-grabbing' : ''
        }`}
      >
        <div
          style={{
            transform: `translate3d(${tx}px, ${ty}px, 0) scale(${scale})`,
            transformOrigin: 'center center',
          }}
          className="relative flex max-h-full max-w-full items-center justify-center"
        >
          <div className="relative inline-block">
            <img
              ref={imgRef}
              src={resourceBlobUrl(resourceId)}
              alt=""
              draggable={false}
              className="block max-h-[55svh] landscape:max-h-[65svh] max-w-full object-contain"
            />

            {showAnswerOverlay && overlayResourceId && (
              <img
                src={resourceBlobUrl(overlayResourceId)}
                alt=""
                draggable={false}
                className="pointer-events-none absolute inset-0 h-full w-full object-fill opacity-70"
              />
            )}

            {/* Existing pin markers for identify mode or post-grading reveal */}
            {(mode === 'identify' || revealPins) &&
              pins.map((pin) => (
                <div
                  key={`pin-marker-${pin.no}`}
                  style={{
                    left: `${pin.x * 100}%`,
                    top: `${pin.y * 100}%`,
                  }}
                  className="pointer-events-none absolute z-10 -translate-x-1/2 -translate-y-1/2"
                >
                  <div
                    className={`flex h-7 w-7 items-center justify-center rounded-full border-2 font-mono text-xs font-bold shadow ${
                      revealPins
                        ? 'border-white bg-ok text-white ring-2 ring-ok/40'
                        : 'border-white bg-accent text-white ring-2 ring-accent/40'
                    }`}
                  >
                    {pin.no}
                  </div>
                  {revealPins && (
                    <div className="mt-1 whitespace-nowrap rounded bg-surface/95 px-1.5 py-0.5 text-center text-[11px] font-medium text-ink shadow">
                      {pin.term_primary}
                    </div>
                  )}
                </div>
              ))}

            {/* User placed clicks for pin_locate */}
            {placedClicks?.map((click) => (
              <div
                key={`click-${click.no}`}
                style={{
                  left: `${click.point.x * 100}%`,
                  top: `${click.point.y * 100}%`,
                }}
                className="pointer-events-none absolute z-20 -translate-x-1/2 -translate-y-1/2"
              >
                <span className="flex h-8 w-8 items-center justify-center rounded-full border-2 border-white bg-warn font-mono text-xs font-bold text-white shadow-md ring-2 ring-warn/50">
                  {click.no}
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}
