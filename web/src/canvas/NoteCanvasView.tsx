import {
  AlertCircle,
  ArrowRight,
  Check,
  CheckCircle2,
  Clock,
  Eye,
  FileSearch,
  GitBranch,
  GripVertical,
  Image as ImageIcon,
  LayoutGrid,
  MapPin,
  Maximize2,
  Save,
  Sigma,
  X,
  ZoomIn,
  ZoomOut,
} from 'lucide-react'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
} from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '../api/client'
import { useUpdateNoteViewSettings } from '../api/hooks/useNotes'
import type {
  CanvasNodeLayout,
  CanvasViewport,
  ContentBlock,
  Edge,
  EdgeType,
  KnowledgeCard,
  Note,
} from '../api/types'
import { KnowledgeCardView } from '../components/KnowledgeCardView'
import {
  CANVAS_VERSION,
  MAX_CANVAS_ZOOM,
  MIN_CANVAS_ZOOM,
  collectOutlineCards,
  computeDeterministicLayout,
  computeEdgeCurve,
  computeFitViewport,
  computePinchViewport,
  resolveCanvasState,
  roundCoord,
  zoomViewportAt,
  type Point2D,
} from './layout'

interface CardEntry {
  card?: KnowledgeCard
  loading: boolean
  error: boolean
}

interface NodeDragState {
  pointerId: number
  cardId: string
  startClientX: number
  startClientY: number
  originX: number
  originY: number
  moved: boolean
}

interface PanState {
  pointerId: number
  startClientX: number
  startClientY: number
  originX: number
  originY: number
  moved: boolean
  tapCardId: string | null
}

interface PinchState {
  startViewport: CanvasViewport
  startDistance: number
  startMidpoint: Point2D
}

function summarizeCardMarkdown(blocks: ContentBlock[]): string | null {
  for (const block of blocks) {
    if (block.type === 'markdown') {
      const cleaned = block.text
        .replace(/^>\s*\[![^\]]+\]\s*/gm, '')
        .replace(/\|/g, ' ')
        .replace(/[#*_`>-]+/g, ' ')
        .replace(/\s+/g, ' ')
        .trim()
      if (cleaned) return cleaned
    }
    if (block.type === 'interactive_pins' && block.pins.length > 0) {
      return block.pins
        .map((p) => (p.term_secondary ? `${p.term_primary} (${p.term_secondary})` : p.term_primary))
        .join(' · ')
    }
    if (block.type === 'math') {
      return block.latex
    }
  }
  return null
}

function blockTypesOf(blocks: ContentBlock[]): ContentBlock['type'][] {
  return Array.from(new Set(blocks.map((b) => b.type)))
}

function edgeStrokeColor(edgeType: EdgeType): string {
  switch (edgeType) {
    case 'prerequisite':
      return 'var(--rf-accent)'
    case 'confusable_with':
      return 'var(--rf-warn)'
    default:
      return 'var(--rf-ink-muted)'
  }
}

function edgeDashArray(edgeType: EdgeType): string | undefined {
  switch (edgeType) {
    case 'prerequisite':
      return undefined
    case 'confusable_with':
      return '7 5'
    default:
      return '3 4'
  }
}

function edgeBadgeClass(edgeType: EdgeType): string {
  switch (edgeType) {
    case 'prerequisite':
      return 'border-accent/40 bg-accent-soft text-accent'
    case 'confusable_with':
      return 'border-warn/40 bg-warn-soft text-warn'
    default:
      return 'border-border bg-subtle text-ink-muted'
  }
}

function pairKey(a: string, b: string): string {
  return a < b ? `${a}|${b}` : `${b}|${a}`
}

export function NoteCanvasView({
  note,
  cardMap,
  edges,
  isOwner,
  onOpenAnchor,
}: {
  note: Note
  cardMap: Map<string, CardEntry>
  edges: Edge[]
  isOwner: boolean
  onOpenAnchor: (anchorId: string) => void
}) {
  const { t } = useTranslation()
  const saveMutation = useUpdateNoteViewSettings(note.id)

  const stageRef = useRef<HTMLDivElement | null>(null)
  const pointersRef = useRef<Map<number, Point2D>>(new Map())
  const panRef = useRef<PanState | null>(null)
  const pinchRef = useRef<PinchState | null>(null)
  const nodeDragRef = useRef<NodeDragState | null>(null)
  const initializedNoteRef = useRef<string | null>(null)
  const autoFittedRef = useRef<boolean>(false)

  const outlineEntries = useMemo(() => collectOutlineCards(note.outline), [note.outline])
  const sectionByCardId = useMemo(() => {
    const map = new Map<string, string | null>()
    for (const entry of outlineEntries) {
      map.set(entry.cardId, entry.sectionHeading)
    }
    return map
  }, [outlineEntries])

  const cardEdges = useMemo(() => {
    const allowed = new Set(outlineEntries.map((e) => e.cardId))
    return edges.filter(
      (edge) =>
        edge.from.kind === 'card' &&
        edge.to.kind === 'card' &&
        edge.from.id !== edge.to.id &&
        allowed.has(edge.from.id) &&
        allowed.has(edge.to.id),
    )
  }, [edges, outlineEntries])

  const initialResolved = useMemo(
    () => resolveCanvasState(note.outline, cardEdges, note.view_settings),
    [note.outline, cardEdges, note.view_settings],
  )

  const [nodes, setNodes] = useState<CanvasNodeLayout[]>(() => initialResolved.nodes)
  const [viewport, setViewport] = useState<CanvasViewport>(() => initialResolved.viewport)
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false)
  const [savedNotice, setSavedNotice] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [selectedCardId, setSelectedCardId] = useState<string | null>(null)
  const [draggingCardId, setDraggingCardId] = useState<string | null>(null)
  const [isPanning, setIsPanning] = useState(false)

  // Re-initialize canvas state when switching to a different note or when edges load before user edits.
  const edgesSignature = useMemo(() => cardEdges.map((e) => e.id).join(','), [cardEdges])
  useEffect(() => {
    const key = `${note.id}:${note.updated_at}:${edgesSignature}`
    if (initializedNoteRef.current === key) return
    if (hasUnsavedChanges && initializedNoteRef.current?.startsWith(`${note.id}:`)) return

    const resolved = resolveCanvasState(note.outline, cardEdges, note.view_settings)
    setNodes(resolved.nodes)
    setViewport(resolved.viewport)
    setHasUnsavedChanges(false)
    setSaveError(null)
    initializedNoteRef.current = key
    autoFittedRef.current = resolved.hasSavedLayout
  }, [note.id, note.updated_at, note.outline, note.view_settings, cardEdges, edgesSignature, hasUnsavedChanges])

  // Auto-fit once after stage mounts if the note has no saved canvas layout.
  useEffect(() => {
    if (autoFittedRef.current || nodes.length === 0) return
    const stage = stageRef.current
    if (!stage) return
    const rect = stage.getBoundingClientRect()
    if (rect.width > 0 && rect.height > 0) {
      setViewport(computeFitViewport(nodes, rect.width, rect.height, rect.width < 500 ? 24 : 56))
      autoFittedRef.current = true
    }
  }, [nodes])

  // Close card drawer with Escape.
  useEffect(() => {
    if (!selectedCardId) return
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') setSelectedCardId(null)
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [selectedCardId])

  const fitView = useCallback(() => {
    const stage = stageRef.current
    if (!stage || nodes.length === 0) return
    const rect = stage.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return
    const next = computeFitViewport(nodes, rect.width, rect.height, rect.width < 500 ? 24 : 56)
    setViewport(next)
    if (isOwner) setHasUnsavedChanges(true)
  }, [nodes, isOwner])

  const zoomStep = useCallback(
    (factor: number) => {
      const stage = stageRef.current
      const rect = stage?.getBoundingClientRect()
      const focal: Point2D = rect
        ? { x: rect.width / 2, y: rect.height / 2 }
        : { x: 400, y: 260 }
      setViewport((prev) => zoomViewportAt(prev, prev.zoom * factor, focal))
      if (isOwner) setHasUnsavedChanges(true)
    },
    [isOwner],
  )

  const handleAutoArrange = useCallback(() => {
    if (!isOwner) return
    const arranged = computeDeterministicLayout(note.outline, cardEdges)
    setNodes(arranged)
    const stage = stageRef.current
    const rect = stage?.getBoundingClientRect()
    if (rect && rect.width > 0 && rect.height > 0) {
      setViewport(computeFitViewport(arranged, rect.width, rect.height, rect.width < 500 ? 24 : 56))
    }
    setHasUnsavedChanges(true)
    setSavedNotice(false)
    setSaveError(null)
  }, [isOwner, note.outline, cardEdges])

  const handleSave = useCallback(async () => {
    if (!isOwner) return
    setSaveError(null)
    setSavedNotice(false)
    try {
      await saveMutation.mutateAsync({
        canvas: {
          version: CANVAS_VERSION,
          nodes: nodes.map((n) => ({
            card_id: n.card_id,
            x: roundCoord(n.x),
            y: roundCoord(n.y),
            w: roundCoord(n.w),
            h: roundCoord(n.h),
          })),
          viewport: {
            x: roundCoord(viewport.x),
            y: roundCoord(viewport.y),
            zoom: roundCoord(viewport.zoom),
          },
        },
      })
      setHasUnsavedChanges(false)
      setSavedNotice(true)
    } catch (err) {
      setSaveError(err instanceof ApiError ? err.message : t('canvas.saveError'))
    }
  }, [isOwner, nodes, viewport, saveMutation, t])

  function focusNode(cardId: string) {
    const target = nodes.find((n) => n.card_id === cardId)
    const stage = stageRef.current
    if (!target || !stage) return
    const rect = stage.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return
    setViewport((prev) => ({
      ...prev,
      x: roundCoord(rect.width / 2 - (target.x + target.w / 2) * prev.zoom),
      y: roundCoord(rect.height / 2 - (target.y + target.h / 2) * prev.zoom),
    }))
    setSelectedCardId(cardId)
  }

  function stagePointFromClient(clientX: number, clientY: number): Point2D {
    const rect = stageRef.current?.getBoundingClientRect()
    if (!rect) return { x: clientX, y: clientY }
    return { x: clientX - rect.left, y: clientY - rect.top }
  }

  function handleStagePointerDown(e: ReactPointerEvent<HTMLDivElement>) {
    if (e.button !== 0 && e.pointerType === 'mouse') return
    e.currentTarget.setPointerCapture(e.pointerId)
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY })

    if (pointersRef.current.size === 2) {
      // Transition cleanly into two-finger pinch-to-zoom.
      nodeDragRef.current = null
      setDraggingCardId(null)
      panRef.current = null
      setIsPanning(false)

      const [p1, p2] = Array.from(pointersRef.current.values())
      const dist = Math.max(1, Math.hypot(p1.x - p2.x, p1.y - p2.y))
      const mid = stagePointFromClient((p1.x + p2.x) / 2, (p1.y + p2.y) / 2)
      pinchRef.current = {
        startViewport: viewport,
        startDistance: dist,
        startMidpoint: mid,
      }
      return
    }

    if (pointersRef.current.size === 1 && !nodeDragRef.current) {
      const nodeEl = (e.target as HTMLElement | null)?.closest<HTMLElement>('[data-canvas-node]')
      panRef.current = {
        pointerId: e.pointerId,
        startClientX: e.clientX,
        startClientY: e.clientY,
        originX: viewport.x,
        originY: viewport.y,
        moved: false,
        tapCardId: nodeEl?.dataset.canvasNode ?? null,
      }
      setIsPanning(true)
    }
  }

  function handleStagePointerMove(e: ReactPointerEvent<HTMLDivElement>) {
    if (!pointersRef.current.has(e.pointerId)) return
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY })

    if (pointersRef.current.size === 2 && pinchRef.current) {
      const [p1, p2] = Array.from(pointersRef.current.values())
      const currentDist = Math.max(1, Math.hypot(p1.x - p2.x, p1.y - p2.y))
      const currentMid = stagePointFromClient((p1.x + p2.x) / 2, (p1.y + p2.y) / 2)
      setViewport(
        computePinchViewport(
          pinchRef.current.startViewport,
          pinchRef.current.startDistance,
          pinchRef.current.startMidpoint,
          currentDist,
          currentMid,
        ),
      )
      if (isOwner) setHasUnsavedChanges(true)
      return
    }

    const drag = nodeDragRef.current
    if (drag && drag.pointerId === e.pointerId && isOwner) {
      const dx = e.clientX - drag.startClientX
      const dy = e.clientY - drag.startClientY
      if (!drag.moved && Math.hypot(dx, dy) > 6) {
        drag.moved = true
        setDraggingCardId(drag.cardId)
      }
      if (drag.moved) {
        const nextX = roundCoord(drag.originX + dx / viewport.zoom)
        const nextY = roundCoord(drag.originY + dy / viewport.zoom)
        setNodes((prev) =>
          prev.map((n) => (n.card_id === drag.cardId ? { ...n, x: nextX, y: nextY } : n)),
        )
        setHasUnsavedChanges(true)
        setSavedNotice(false)
      }
      return
    }

    const pan = panRef.current
    if (pan && pan.pointerId === e.pointerId) {
      const dx = e.clientX - pan.startClientX
      const dy = e.clientY - pan.startClientY
      if (!pan.moved && Math.hypot(dx, dy) > 6) {
        pan.moved = true
      }
      if (pan.moved) {
        setViewport((prev) => ({
          ...prev,
          x: roundCoord(pan.originX + dx),
          y: roundCoord(pan.originY + dy),
        }))
        if (isOwner) {
          setHasUnsavedChanges(true)
        }
      }
    }
  }

  function handleStagePointerUp(e: ReactPointerEvent<HTMLDivElement>) {
    const drag = nodeDragRef.current
    if (drag && drag.pointerId === e.pointerId) {
      const wasTap = !drag.moved
      const targetId = drag.cardId
      nodeDragRef.current = null
      setDraggingCardId(null)
      if (wasTap) {
        setSelectedCardId(targetId)
      }
    }

    const pan = panRef.current
    if (pan && pan.pointerId === e.pointerId) {
      const tapCardId = !pan.moved ? pan.tapCardId : null
      panRef.current = null
      setIsPanning(false)
      if (tapCardId) {
        setSelectedCardId(tapCardId)
      }
    }

    pointersRef.current.delete(e.pointerId)
    if (pointersRef.current.size < 2) {
      pinchRef.current = null
    }
  }

  function handleNodePointerDown(e: ReactPointerEvent<HTMLElement>, node: CanvasNodeLayout) {
    if (e.button !== 0 && e.pointerType === 'mouse') return
    // Let a second finger bubble up to the stage for pinch-to-zoom.
    if (pointersRef.current.size >= 1) return

    if (!isOwner) {
      // Non-owners cannot drag cards; let background pan work if they drag, or inspect on click.
      return
    }

    e.stopPropagation()
    stageRef.current?.setPointerCapture(e.pointerId)
    pointersRef.current.set(e.pointerId, { x: e.clientX, y: e.clientY })
    nodeDragRef.current = {
      pointerId: e.pointerId,
      cardId: node.card_id,
      startClientX: e.clientX,
      startClientY: e.clientY,
      originX: node.x,
      originY: node.y,
      moved: false,
    }
  }

  function handleWheel(e: ReactWheelEvent<HTMLDivElement>) {
    e.preventDefault()
    const focal = stagePointFromClient(e.clientX, e.clientY)
    const intensity = e.ctrlKey ? 0.01 : 0.0018
    const factor = Math.exp(-e.deltaY * intensity)
    setViewport((prev) => zoomViewportAt(prev, prev.zoom * factor, focal))
    if (isOwner) setHasUnsavedChanges(true)
  }

  const nodeById = useMemo(() => {
    const map = new Map<string, CanvasNodeLayout>()
    for (const node of nodes) map.set(node.card_id, node)
    return map
  }, [nodes])

  const renderedEdges = useMemo(() => {
    const groupCounts = new Map<string, number>()
    for (const edge of cardEdges) {
      const key = pairKey(edge.from.id, edge.to.id)
      groupCounts.set(key, (groupCounts.get(key) ?? 0) + 1)
    }
    const groupSeen = new Map<string, number>()

    return cardEdges
      .map((edge) => {
        const fromNode = nodeById.get(edge.from.id)
        const toNode = nodeById.get(edge.to.id)
        if (!fromNode || !toNode) return null
        const key = pairKey(edge.from.id, edge.to.id)
        const idx = groupSeen.get(key) ?? 0
        groupSeen.set(key, idx + 1)
        const count = groupCounts.get(key) ?? 1
        const geometry = computeEdgeCurve(fromNode, toNode, idx, count)
        return { edge, geometry }
      })
      .filter((item): item is NonNullable<typeof item> => item !== null)
  }, [cardEdges, nodeById])

  const presentEdgeTypes = useMemo(() => {
    const types = new Set<EdgeType>()
    for (const edge of cardEdges) types.add(edge.edge_type)
    return Array.from(types)
  }, [cardEdges])

  const selectedCardEntry = selectedCardId ? cardMap.get(selectedCardId) : undefined
  const selectedCardConnections = useMemo(() => {
    if (!selectedCardId) return []
    return cardEdges.filter(
      (edge) => edge.from.id === selectedCardId || edge.to.id === selectedCardId,
    )
  }, [cardEdges, selectedCardId])

  if (outlineEntries.length === 0) {
    return (
      <div className="flex min-h-[360px] flex-col items-center justify-center rounded-2xl border border-border bg-surface p-8 text-center">
        <LayoutGrid className="h-9 w-9 text-ink-muted" />
        <h2 className="mt-3 text-base font-semibold text-ink">{t('canvas.emptyTitle')}</h2>
        <p className="mt-1 max-w-sm text-sm text-ink-muted">{t('canvas.emptyDesc')}</p>
      </div>
    )
  }

  const gridSize = Math.max(14, Math.round(24 * viewport.zoom))

  return (
    <div className="flex flex-col overflow-hidden rounded-2xl border border-border bg-surface shadow-sm">
      {/* Canvas Toolbar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-surface px-3 py-2 sm:px-4">
        {/* Zoom & Fit controls */}
        <div className="flex flex-wrap items-center gap-1.5">
          <div className="inline-flex items-center rounded-lg border border-border bg-canvas p-0.5">
            <button
              type="button"
              onClick={() => zoomStep(1 / 1.25)}
              disabled={viewport.zoom <= MIN_CANVAS_ZOOM + 0.001}
              aria-label={t('canvas.zoomOut')}
              className="flex min-h-10 min-w-10 items-center justify-center rounded-md text-ink hover:bg-subtle disabled:opacity-40"
            >
              <ZoomOut className="h-4 w-4" />
            </button>
            <span className="min-w-12 px-1 text-center font-mono text-xs font-medium text-ink-muted">
              {Math.round(viewport.zoom * 100)}%
            </span>
            <button
              type="button"
              onClick={() => zoomStep(1.25)}
              disabled={viewport.zoom >= MAX_CANVAS_ZOOM - 0.001}
              aria-label={t('canvas.zoomIn')}
              className="flex min-h-10 min-w-10 items-center justify-center rounded-md text-ink hover:bg-subtle disabled:opacity-40"
            >
              <ZoomIn className="h-4 w-4" />
            </button>
          </div>

          <button
            type="button"
            onClick={fitView}
            className="inline-flex min-h-10 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
          >
            <Maximize2 className="h-3.5 w-3.5" />
            <span>{t('canvas.fitView')}</span>
          </button>

          {presentEdgeTypes.length > 0 && (
            <div className="hidden items-center gap-1.5 pl-1 md:flex">
              {presentEdgeTypes.map((edgeType) => (
                <span
                  key={edgeType}
                  className={`inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-[11px] font-medium ${edgeBadgeClass(edgeType)}`}
                >
                  <GitBranch className="h-3 w-3" />
                  <span>{t(`canvas.edgeTypes.${edgeType}`)}</span>
                </span>
              ))}
            </div>
          )}
        </div>

        {/* Right: Owner actions OR Learner read-only badge */}
        <div className="flex flex-wrap items-center gap-2">
          {isOwner ? (
            <>
              <button
                type="button"
                onClick={handleAutoArrange}
                className="inline-flex min-h-10 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
              >
                <LayoutGrid className="h-3.5 w-3.5" />
                <span>{t('canvas.autoArrange')}</span>
              </button>

              <button
                type="button"
                onClick={() => void handleSave()}
                disabled={saveMutation.isPending}
                className="inline-flex min-h-10 items-center gap-1.5 rounded-lg bg-accent px-3.5 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
              >
                {savedNotice && !hasUnsavedChanges ? (
                  <Check className="h-3.5 w-3.5" />
                ) : (
                  <Save className="h-3.5 w-3.5" />
                )}
                <span>
                  {saveMutation.isPending
                    ? t('canvas.saving')
                    : savedNotice && !hasUnsavedChanges
                      ? t('canvas.saved')
                      : t('canvas.saveLayout')}
                </span>
                {hasUnsavedChanges && !saveMutation.isPending && (
                  <span
                    className="h-2 w-2 rounded-full bg-white/90"
                    aria-label={t('canvas.unsavedChanges')}
                  />
                )}
              </button>
            </>
          ) : (
            <span className="inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-border bg-subtle px-2.5 text-xs font-medium text-ink-muted">
              <Eye className="h-3.5 w-3.5" />
              <span>{t('canvas.viewOnly')}</span>
            </span>
          )}
        </div>
      </div>

      {saveError && (
        <div
          role="alert"
          className="flex items-center justify-between gap-2 border-b border-bad/30 bg-bad-soft px-4 py-2 text-xs text-bad"
        >
          <div className="flex items-center gap-1.5">
            <AlertCircle className="h-4 w-4 shrink-0" />
            <span>{saveError}</span>
          </div>
          <button
            type="button"
            onClick={() => setSaveError(null)}
            aria-label={t('common.close')}
            className="rounded p-1 hover:bg-bad/10"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      {/* Interactive 2D Canvas Stage */}
      <div
        ref={stageRef}
        role="region"
        aria-label={t('canvas.stageLabel')}
        onPointerDown={handleStagePointerDown}
        onPointerMove={handleStagePointerMove}
        onPointerUp={handleStagePointerUp}
        onPointerCancel={handleStagePointerUp}
        onWheel={handleWheel}
        style={{
          touchAction: 'none',
          backgroundImage:
            'radial-gradient(circle, color-mix(in srgb, var(--rf-border) 85%, transparent) 1.25px, transparent 1.25px)',
          backgroundSize: `${gridSize}px ${gridSize}px`,
          backgroundPosition: `${viewport.x}px ${viewport.y}px`,
        }}
        className={`relative h-[66dvh] min-h-[440px] w-full overflow-hidden bg-canvas select-none lg:h-[calc(100dvh-12.5rem)] ${
          isPanning ? 'cursor-grabbing' : 'cursor-grab'
        }`}
      >
        {/* World Transform Layer */}
        <div
          style={{
            transform: `translate3d(${viewport.x}px, ${viewport.y}px, 0) scale(${viewport.zoom})`,
            transformOrigin: '0 0',
          }}
          className="absolute top-0 left-0 h-0 w-0"
        >
          {/* Typed Edges SVG Layer */}
          <svg
            aria-hidden="true"
            className="pointer-events-none absolute top-0 left-0 overflow-visible"
            width="1"
            height="1"
          >
            <defs>
              <marker
                id="rf-arrow-prerequisite"
                viewBox="0 0 10 10"
                refX="8.5"
                refY="5"
                markerWidth="7"
                markerHeight="7"
                orient="auto-start-reverse"
              >
                <path d="M 0 1.5 L 9 5 L 0 8.5 z" fill="var(--rf-accent)" />
              </marker>
              <marker
                id="rf-arrow-confusable"
                viewBox="0 0 10 10"
                refX="8.5"
                refY="5"
                markerWidth="6.5"
                markerHeight="6.5"
                orient="auto-start-reverse"
              >
                <path d="M 0 1.5 L 9 5 L 0 8.5 z" fill="var(--rf-warn)" />
              </marker>
              <marker
                id="rf-arrow-default"
                viewBox="0 0 10 10"
                refX="8.5"
                refY="5"
                markerWidth="6.5"
                markerHeight="6.5"
                orient="auto-start-reverse"
              >
                <path d="M 0 1.5 L 9 5 L 0 8.5 z" fill="var(--rf-ink-muted)" />
              </marker>
            </defs>

            {renderedEdges.map(({ edge, geometry }) => {
              const stroke = edgeStrokeColor(edge.edge_type)
              const dash = edgeDashArray(edge.edge_type)
              const markerEnd =
                edge.edge_type === 'prerequisite'
                  ? 'url(#rf-arrow-prerequisite)'
                  : edge.edge_type === 'confusable_with'
                    ? 'url(#rf-arrow-confusable)'
                    : 'url(#rf-arrow-default)'
              const markerStart =
                edge.edge_type === 'confusable_with' ? 'url(#rf-arrow-confusable)' : undefined

              return (
                <path
                  key={edge.id}
                  d={geometry.path}
                  fill="none"
                  stroke={stroke}
                  strokeWidth={2.25}
                  strokeDasharray={dash}
                  strokeLinecap="round"
                  markerStart={markerStart}
                  markerEnd={markerEnd}
                />
              )
            })}
          </svg>

          {/* Edge Type Labels */}
          {renderedEdges.map(({ edge, geometry }) => (
            <div
              key={`edge-label-${edge.id}`}
              style={{
                left: `${geometry.label.x}px`,
                top: `${geometry.label.y}px`,
              }}
              className={`pointer-events-none absolute -translate-x-1/2 -translate-y-1/2 whitespace-nowrap rounded-full border px-2.5 py-0.5 text-[11px] font-medium shadow-xs ${edgeBadgeClass(edge.edge_type)}`}
            >
              {t(`canvas.edgeTypes.${edge.edge_type}`)}
            </div>
          ))}

          {/* Knowledge Card Nodes */}
          {nodes.map((node) => {
            const entry = cardMap.get(node.card_id)
            const card = entry?.card
            const sectionHeading = sectionByCardId.get(node.card_id)
            const isSelected = selectedCardId === node.card_id
            const isDragging = draggingCardId === node.card_id
            const summary = card ? summarizeCardMarkdown(card.blocks) : null
            const blockTypes = card ? blockTypesOf(card.blocks) : []

            return (
              <article
                key={node.card_id}
                data-canvas-node={node.card_id}
                onPointerDown={(e) => handleNodePointerDown(e, node)}
                style={{
                  left: `${node.x}px`,
                  top: `${node.y}px`,
                  width: `${node.w}px`,
                  minHeight: `${node.h}px`,
                }}
                className={`absolute flex flex-col justify-between rounded-xl border bg-surface p-4 transition-shadow ${
                  isSelected
                    ? 'border-accent ring-2 ring-accent/35 shadow-lg'
                    : 'border-border shadow-sm hover:border-accent/60 hover:shadow-md'
                } ${isDragging ? 'z-20 cursor-grabbing shadow-xl' : isOwner ? 'cursor-grab' : 'cursor-pointer'}`}
              >
                <div>
                  <div className="flex items-start justify-between gap-2">
                    {sectionHeading ? (
                      <span className="max-w-[82%] truncate rounded-md bg-subtle px-2 py-0.5 text-[11px] font-medium text-ink-muted">
                        {sectionHeading}
                      </span>
                    ) : (
                      <span />
                    )}

                    {isOwner && (
                      <span
                        aria-hidden="true"
                        className="rounded p-0.5 text-ink-muted/70"
                      >
                        <GripVertical className="h-4 w-4" />
                      </span>
                    )}
                  </div>

                  {!entry || entry.loading ? (
                    <p className="mt-3 text-sm text-ink-muted">{t('common.loading')}</p>
                  ) : entry.error || !card ? (
                    <p className="mt-3 text-sm text-bad">{t('reading.missingCard')}</p>
                  ) : (
                    <>
                      <h3 className="mt-2 line-clamp-2 text-base font-semibold leading-snug tracking-tight text-ink">
                        {card.title}
                      </h3>

                      {summary && (
                        <p className="mt-1.5 line-clamp-2 text-xs leading-relaxed text-ink-muted">
                          {summary}
                        </p>
                      )}

                      {blockTypes.length > 0 && (
                        <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
                          {blockTypes.map((blockType) => (
                            <span
                              key={blockType}
                              className="inline-flex items-center gap-1 rounded border border-border bg-canvas px-1.5 py-0.5 text-[11px] text-ink-muted"
                            >
                              {blockType === 'interactive_pins' && <MapPin className="h-3 w-3 text-accent" />}
                              {blockType === 'math' && <Sigma className="h-3 w-3 text-accent" />}
                              {blockType === 'image_occlusion' && <ImageIcon className="h-3 w-3 text-accent" />}
                              <span>{t(`canvas.blocks.${blockType}`)}</span>
                            </span>
                          ))}
                          {card.anchor_ids.length > 0 && (
                            <span className="inline-flex items-center gap-1 rounded border border-border bg-canvas px-1.5 py-0.5 text-[11px] text-ink-muted">
                              <FileSearch className="h-3 w-3 text-accent" />
                              <span>{card.anchor_ids.length}</span>
                            </span>
                          )}
                        </div>
                      )}
                    </>
                  )}
                </div>

                {card && (
                  <footer className="mt-3 flex items-center justify-between gap-2 border-t border-border pt-2.5">
                    <span
                      className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium ${
                        card.verification === 'verified'
                          ? 'bg-ok-soft text-ok'
                          : 'bg-warn-soft text-warn'
                      }`}
                    >
                      {card.verification === 'verified' ? (
                        <CheckCircle2 className="h-3 w-3" />
                      ) : (
                        <Clock className="h-3 w-3" />
                      )}
                      <span>
                        {card.verification === 'verified'
                          ? t('common.verified')
                          : t('common.unchecked')}
                      </span>
                    </span>

                    <button
                      type="button"
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => setSelectedCardId(card.id)}
                      className="inline-flex min-h-8 items-center gap-1 rounded-md px-2 text-xs font-medium text-accent hover:bg-accent-soft"
                    >
                      <span>{t('canvas.inspectCard')}</span>
                      <ArrowRight className="h-3.5 w-3.5" />
                    </button>
                  </footer>
                )}
              </article>
            )
          })}
        </div>

        {/* Quiet Bottom Hint Pill */}
        <div className="pointer-events-none absolute right-3 bottom-3 left-3 flex justify-center sm:justify-end">
          <span className="rounded-full border border-border/80 bg-surface/90 px-3 py-1 text-[11px] text-ink-muted shadow-xs backdrop-blur-xs">
            {isOwner ? t('canvas.ownerHint') : t('canvas.viewOnlyHint')}
          </span>
        </div>
      </div>

      {/* Card Detail Slide-Over / Bottom-Sheet Drawer */}
      {selectedCardId && (
        <div
          className="fixed inset-0 z-40 flex items-end justify-end md:items-stretch"
          role="dialog"
          aria-modal="true"
          aria-label={selectedCardEntry?.card?.title ?? t('canvas.cardDrawerTitle')}
        >
          <button
            type="button"
            aria-label={t('common.close')}
            onClick={() => setSelectedCardId(null)}
            className="fixed inset-0 bg-black/45 backdrop-blur-[1px]"
          />

          <div className="relative z-10 flex max-h-[86dvh] w-full flex-col rounded-t-2xl border-t border-border bg-canvas shadow-2xl md:h-full md:max-h-none md:max-w-2xl md:rounded-t-none md:border-t-0 md:border-l">
            <div className="flex items-center justify-between border-b border-border bg-surface px-4 py-3">
              <div className="min-w-0">
                <h2 className="truncate text-sm font-semibold text-ink">
                  {selectedCardEntry?.card?.title ?? t('canvas.cardDrawerTitle')}
                </h2>
                {sectionByCardId.get(selectedCardId) && (
                  <p className="truncate text-xs text-ink-muted">
                    {sectionByCardId.get(selectedCardId)}
                  </p>
                )}
              </div>

              <button
                type="button"
                onClick={() => setSelectedCardId(null)}
                aria-label={t('common.close')}
                className="flex min-h-11 min-w-11 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink"
              >
                <X className="h-5 w-5" />
              </button>
            </div>

            <div className="flex-1 space-y-4 overflow-y-auto p-4 sm:p-5">
              {!selectedCardEntry || selectedCardEntry.loading ? (
                <div className="py-10 text-center text-sm text-ink-muted">
                  {t('common.loading')}
                </div>
              ) : selectedCardEntry.error || !selectedCardEntry.card ? (
                <div className="rounded-xl border border-bad/40 bg-bad-soft p-4 text-sm text-bad">
                  {t('reading.missingCard')}
                </div>
              ) : (
                <KnowledgeCardView
                  card={selectedCardEntry.card}
                  onOpenAnchor={onOpenAnchor}
                  showCardLink
                />
              )}

              {selectedCardConnections.length > 0 && (
                <section className="rounded-xl border border-border bg-surface p-4">
                  <h3 className="flex items-center gap-1.5 text-xs font-semibold text-ink-muted">
                    <GitBranch className="h-3.5 w-3.5 text-accent" />
                    <span>{t('canvas.connectedCards')}</span>
                  </h3>
                  <ul className="mt-2.5 space-y-1.5">
                    {selectedCardConnections.map((edge) => {
                      const isOutgoing = edge.from.id === selectedCardId
                      const peerId = isOutgoing ? edge.to.id : edge.from.id
                      const peerTitle = cardMap.get(peerId)?.card?.title ?? peerId.slice(-8)
                      const relationLabel =
                        edge.edge_type === 'prerequisite'
                          ? isOutgoing
                            ? t('canvas.edgeDirectionFrom')
                            : t('canvas.edgeDirectionTo')
                          : t(`canvas.edgeTypes.${edge.edge_type}`)

                      return (
                        <li key={edge.id}>
                          <button
                            type="button"
                            onClick={() => focusNode(peerId)}
                            className="flex min-h-11 w-full items-center justify-between gap-2 rounded-lg border border-border bg-canvas px-3 py-2 text-left text-xs hover:border-accent hover:bg-accent-soft/40"
                          >
                            <span className="truncate font-medium text-ink">{peerTitle}</span>
                            <span
                              className={`shrink-0 rounded-full border px-2 py-0.5 text-[11px] font-medium ${edgeBadgeClass(edge.edge_type)}`}
                            >
                              {relationLabel}
                            </span>
                          </button>
                        </li>
                      )
                    })}
                  </ul>
                </section>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
