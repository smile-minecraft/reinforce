import type {
  CanvasNodeLayout,
  CanvasViewport,
  CanvasViewSettings,
  Edge,
  OutlineNode,
} from '../api/types'

export const CANVAS_VERSION = 1 as const
export const DEFAULT_NODE_WIDTH = 320
export const DEFAULT_NODE_HEIGHT = 200
export const MIN_CANVAS_ZOOM = 0.1
export const MAX_CANVAS_ZOOM = 4.0
export const MAX_CANVAS_NODES = 2000

const CANVAS_PADDING_X = 48
const CANVAS_PADDING_Y = 48
const NODE_GAP_X = 96
const NODE_GAP_Y = 56
const MAX_SECTION_COLUMNS = 3

export interface OutlineCardEntry {
  cardId: string
  outlineIndex: number
  sectionIndex: number
  sectionHeading: string | null
}

export interface ResolvedCanvasState {
  nodes: CanvasNodeLayout[]
  viewport: CanvasViewport
  hasSavedLayout: boolean
}

export interface Point2D {
  x: number
  y: number
}

export interface EdgeCurveGeometry {
  path: string
  start: Point2D
  end: Point2D
  label: Point2D
}

export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom)) return 1
  return Math.min(MAX_CANVAS_ZOOM, Math.max(MIN_CANVAS_ZOOM, zoom))
}

export function roundCoord(value: number): number {
  return Math.round(value * 100) / 100
}

/**
 * Extracts unique KnowledgeCard entries from a Note outline in deterministic pre-order.
 */
export function collectOutlineCards(outline: OutlineNode[]): OutlineCardEntry[] {
  const entries: OutlineCardEntry[] = []
  const seen = new Set<string>()
  let nextSectionIndex = 0

  function visit(nodes: OutlineNode[], currentSectionIndex: number, currentHeading: string | null) {
    for (const node of nodes) {
      if (node.type === 'section') {
        const idx = nextSectionIndex++
        visit(node.children, idx, node.heading)
      } else if (!seen.has(node.card_id)) {
        seen.add(node.card_id)
        entries.push({
          cardId: node.card_id,
          outlineIndex: entries.length,
          sectionIndex: currentSectionIndex,
          sectionHeading: currentHeading,
        })
      }
    }
  }

  visit(outline, -1, null)
  return entries
}

/**
 * Parses and validates the `canvas` sub-key from `note.view_settings`.
 * Returns null if absent or malformed.
 */
export function parseCanvasViewSettings(
  viewSettings: Record<string, unknown> | null | undefined,
): CanvasViewSettings | null {
  if (!viewSettings || typeof viewSettings !== 'object') return null
  const raw = viewSettings.canvas
  if (!raw || typeof raw !== 'object') return null

  const candidate = raw as Record<string, unknown>
  if (candidate.version !== CANVAS_VERSION) return null
  if (!Array.isArray(candidate.nodes) || candidate.nodes.length > MAX_CANVAS_NODES) return null

  const rawViewport = candidate.viewport as Record<string, unknown> | null | undefined
  if (!rawViewport || typeof rawViewport !== 'object') return null
  const vx = rawViewport.x
  const vy = rawViewport.y
  const vz = rawViewport.zoom
  if (
    typeof vx !== 'number' ||
    typeof vy !== 'number' ||
    typeof vz !== 'number' ||
    !Number.isFinite(vx) ||
    !Number.isFinite(vy) ||
    !Number.isFinite(vz) ||
    vz < MIN_CANVAS_ZOOM ||
    vz > MAX_CANVAS_ZOOM
  ) {
    return null
  }

  const nodes: CanvasNodeLayout[] = []
  const seen = new Set<string>()
  for (const item of candidate.nodes) {
    if (!item || typeof item !== 'object') return null
    const rec = item as Record<string, unknown>
    if (typeof rec.card_id !== 'string' || !rec.card_id || seen.has(rec.card_id)) return null
    if (
      typeof rec.x !== 'number' ||
      typeof rec.y !== 'number' ||
      typeof rec.w !== 'number' ||
      typeof rec.h !== 'number' ||
      !Number.isFinite(rec.x) ||
      !Number.isFinite(rec.y) ||
      !Number.isFinite(rec.w) ||
      !Number.isFinite(rec.h) ||
      rec.w <= 0 ||
      rec.h <= 0
    ) {
      return null
    }
    seen.add(rec.card_id)
    nodes.push({
      card_id: rec.card_id,
      x: rec.x,
      y: rec.y,
      w: rec.w,
      h: rec.h,
    })
  }

  return {
    version: CANVAS_VERSION,
    nodes,
    viewport: { x: vx, y: vy, zoom: vz },
  }
}

/**
 * Computes a deterministic 2D layout for every unique card in the note outline.
 *
 * - When `prerequisite` edges connect cards in the outline, cards are assigned
 *   longest-path layers left-to-right (breaking any cycles deterministically by
 *   outline order) and stacked vertically within each layer.
 * - Otherwise, cards are grouped by their outline section into clean rows of up
 *   to 3 cards.
 *
 * Output is strictly independent of the input order of `edges`.
 */
export function computeDeterministicLayout(
  outline: OutlineNode[],
  edges: Edge[],
): CanvasNodeLayout[] {
  const entries = collectOutlineCards(outline)
  if (entries.length === 0) return []

  const entryById = new Map<string, OutlineCardEntry>()
  for (const entry of entries) {
    entryById.set(entry.cardId, entry)
  }

  // Extract canonical, sorted prerequisite pairs within the outline.
  const prereqPairs: Array<{ from: string; to: string }> = []
  const seenPairs = new Set<string>()
  for (const edge of edges) {
    if (edge.edge_type !== 'prerequisite') continue
    if (edge.from.kind !== 'card' || edge.to.kind !== 'card') continue
    const fromId = edge.from.id
    const toId = edge.to.id
    if (fromId === toId || !entryById.has(fromId) || !entryById.has(toId)) continue
    const pairKey = `${fromId}->${toId}`
    if (!seenPairs.has(pairKey)) {
      seenPairs.add(pairKey)
      prereqPairs.push({ from: fromId, to: toId })
    }
  }
  prereqPairs.sort((a, b) => {
    const fromDiff = entryById.get(a.from)!.outlineIndex - entryById.get(b.from)!.outlineIndex
    if (fromDiff !== 0) return fromDiff
    return entryById.get(a.to)!.outlineIndex - entryById.get(b.to)!.outlineIndex
  })

  if (prereqPairs.length > 0) {
    return layoutByPrerequisiteLayers(entries, entryById, prereqPairs)
  }

  return layoutByOutlineSections(entries)
}

function layoutByPrerequisiteLayers(
  entries: OutlineCardEntry[],
  entryById: Map<string, OutlineCardEntry>,
  prereqPairs: Array<{ from: string; to: string }>,
): CanvasNodeLayout[] {
  const outgoing = new Map<string, string[]>()
  const inDegree = new Map<string, number>()
  for (const entry of entries) {
    outgoing.set(entry.cardId, [])
    inDegree.set(entry.cardId, 0)
  }
  for (const pair of prereqPairs) {
    outgoing.get(pair.from)!.push(pair.to)
    inDegree.set(pair.to, (inDegree.get(pair.to) ?? 0) + 1)
  }

  // Deterministic Kahn topological order with cycle-breaking by outlineIndex.
  const remaining = new Set(entries.map((e) => e.cardId))
  const topoOrder: string[] = []
  while (remaining.size > 0) {
    const ready: string[] = []
    for (const id of remaining) {
      if ((inDegree.get(id) ?? 0) === 0) ready.push(id)
    }
    ready.sort((a, b) => entryById.get(a)!.outlineIndex - entryById.get(b)!.outlineIndex)

    const nextId =
      ready[0] ??
      Array.from(remaining).sort(
        (a, b) => entryById.get(a)!.outlineIndex - entryById.get(b)!.outlineIndex,
      )[0]

    remaining.delete(nextId)
    topoOrder.push(nextId)
    for (const target of outgoing.get(nextId) ?? []) {
      if (remaining.has(target)) {
        inDegree.set(target, Math.max(0, (inDegree.get(target) ?? 0) - 1))
      }
    }
  }

  // Longest-path layer assignment along forward edges in topoOrder.
  const topoPos = new Map<string, number>()
  topoOrder.forEach((id, i) => topoPos.set(id, i))

  const layerById = new Map<string, number>()
  for (const id of topoOrder) {
    layerById.set(id, 0)
  }
  for (const id of topoOrder) {
    const currentLayer = layerById.get(id) ?? 0
    for (const target of outgoing.get(id) ?? []) {
      if ((topoPos.get(target) ?? 0) > (topoPos.get(id) ?? 0)) {
        const candidate = currentLayer + 1
        if (candidate > (layerById.get(target) ?? 0)) {
          layerById.set(target, candidate)
        }
      }
    }
  }

  const layers = new Map<number, OutlineCardEntry[]>()
  let maxLayer = 0
  for (const entry of entries) {
    const layer = layerById.get(entry.cardId) ?? 0
    if (layer > maxLayer) maxLayer = layer
    const bucket = layers.get(layer) ?? []
    bucket.push(entry)
    layers.set(layer, bucket)
  }

  let maxColHeight = 0
  for (const bucket of layers.values()) {
    bucket.sort(
      (a, b) =>
        a.sectionIndex - b.sectionIndex ||
        a.outlineIndex - b.outlineIndex ||
        a.cardId.localeCompare(b.cardId),
    )
    const height =
      bucket.length * DEFAULT_NODE_HEIGHT + Math.max(0, bucket.length - 1) * NODE_GAP_Y
    if (height > maxColHeight) maxColHeight = height
  }

  const posById = new Map<string, CanvasNodeLayout>()
  for (let layer = 0; layer <= maxLayer; layer += 1) {
    const bucket = layers.get(layer)
    if (!bucket || bucket.length === 0) continue
    const colHeight =
      bucket.length * DEFAULT_NODE_HEIGHT + Math.max(0, bucket.length - 1) * NODE_GAP_Y
    const yOffset = CANVAS_PADDING_Y + Math.round((maxColHeight - colHeight) / 2)
    const x = CANVAS_PADDING_X + layer * (DEFAULT_NODE_WIDTH + NODE_GAP_X)
    bucket.forEach((entry, row) => {
      posById.set(entry.cardId, {
        card_id: entry.cardId,
        x,
        y: yOffset + row * (DEFAULT_NODE_HEIGHT + NODE_GAP_Y),
        w: DEFAULT_NODE_WIDTH,
        h: DEFAULT_NODE_HEIGHT,
      })
    })
  }

  return entries.map((entry) => posById.get(entry.cardId)!)
}

function layoutByOutlineSections(entries: OutlineCardEntry[]): CanvasNodeLayout[] {
  const sections: OutlineCardEntry[][] = []
  let currentGroup: OutlineCardEntry[] = []
  let lastSectionIndex: number | null = null

  for (const entry of entries) {
    if (lastSectionIndex !== null && entry.sectionIndex !== lastSectionIndex) {
      sections.push(currentGroup)
      currentGroup = []
    }
    currentGroup.push(entry)
    lastSectionIndex = entry.sectionIndex
  }
  if (currentGroup.length > 0) sections.push(currentGroup)

  const layout: CanvasNodeLayout[] = []
  let cursorY = CANVAS_PADDING_Y

  for (const group of sections) {
    const rows = Math.ceil(group.length / MAX_SECTION_COLUMNS)
    group.forEach((entry, idx) => {
      const col = idx % MAX_SECTION_COLUMNS
      const row = Math.floor(idx / MAX_SECTION_COLUMNS)
      layout.push({
        card_id: entry.cardId,
        x: CANVAS_PADDING_X + col * (DEFAULT_NODE_WIDTH + NODE_GAP_X),
        y: cursorY + row * (DEFAULT_NODE_HEIGHT + NODE_GAP_Y),
        w: DEFAULT_NODE_WIDTH,
        h: DEFAULT_NODE_HEIGHT,
      })
    })
    cursorY += rows * (DEFAULT_NODE_HEIGHT + NODE_GAP_Y) + Math.round(NODE_GAP_Y * 0.5)
  }

  return layout
}

/**
 * Resolves initial canvas nodes and viewport for a Note:
 * - Uses saved canvas layout when valid, preserving saved positions for outline cards,
 *   dropping cards removed from the outline, and deterministically appending any newly
 *   added outline cards below the saved bounding box.
 * - Falls back to deterministic auto-layout when no valid saved layout exists.
 */
export function resolveCanvasState(
  outline: OutlineNode[],
  edges: Edge[],
  viewSettings: Record<string, unknown> | null | undefined,
): ResolvedCanvasState {
  const entries = collectOutlineCards(outline)
  const saved = parseCanvasViewSettings(viewSettings)

  if (!saved) {
    return {
      nodes: computeDeterministicLayout(outline, edges),
      viewport: { x: 0, y: 0, zoom: 1 },
      hasSavedLayout: false,
    }
  }

  const savedById = new Map<string, CanvasNodeLayout>()
  for (const node of saved.nodes) {
    savedById.set(node.card_id, node)
  }

  const presentSaved: CanvasNodeLayout[] = []
  const missingEntries: OutlineCardEntry[] = []

  for (const entry of entries) {
    const existing = savedById.get(entry.cardId)
    if (existing) {
      presentSaved.push({ ...existing })
    } else {
      missingEntries.push(entry)
    }
  }

  if (presentSaved.length === 0) {
    return {
      nodes: computeDeterministicLayout(outline, edges),
      viewport: { ...saved.viewport },
      hasSavedLayout: true,
    }
  }

  if (missingEntries.length === 0) {
    return {
      nodes: presentSaved,
      viewport: { ...saved.viewport },
      hasSavedLayout: true,
    }
  }

  // Place newly added cards deterministically below existing saved nodes.
  const minX = Math.min(...presentSaved.map((n) => n.x))
  const maxBottom = Math.max(...presentSaved.map((n) => n.y + n.h))
  const startY = roundCoord(maxBottom + NODE_GAP_Y)

  const appendedById = new Map<string, CanvasNodeLayout>()
  missingEntries.forEach((entry, idx) => {
    const col = idx % MAX_SECTION_COLUMNS
    const row = Math.floor(idx / MAX_SECTION_COLUMNS)
    appendedById.set(entry.cardId, {
      card_id: entry.cardId,
      x: roundCoord(minX + col * (DEFAULT_NODE_WIDTH + NODE_GAP_X)),
      y: roundCoord(startY + row * (DEFAULT_NODE_HEIGHT + NODE_GAP_Y)),
      w: DEFAULT_NODE_WIDTH,
      h: DEFAULT_NODE_HEIGHT,
    })
  })

  const orderedNodes = entries.map(
    (entry) => savedById.get(entry.cardId) ?? appendedById.get(entry.cardId)!,
  )

  return {
    nodes: orderedNodes,
    viewport: { ...saved.viewport },
    hasSavedLayout: true,
  }
}

/**
 * Computes a viewport that fits all nodes comfortably inside the stage bounds.
 */
export function computeFitViewport(
  nodes: CanvasNodeLayout[],
  stageWidth: number,
  stageHeight: number,
  padding = 48,
): CanvasViewport {
  if (nodes.length === 0 || stageWidth <= 0 || stageHeight <= 0) {
    return { x: 0, y: 0, zoom: 1 }
  }

  const minX = Math.min(...nodes.map((n) => n.x))
  const minY = Math.min(...nodes.map((n) => n.y))
  const maxX = Math.max(...nodes.map((n) => n.x + n.w))
  const maxY = Math.max(...nodes.map((n) => n.y + n.h))

  const contentWidth = Math.max(1, maxX - minX)
  const contentHeight = Math.max(1, maxY - minY)
  const availWidth = Math.max(80, stageWidth - padding * 2)
  const availHeight = Math.max(80, stageHeight - padding * 2)

  const rawZoom = Math.min(availWidth / contentWidth, availHeight / contentHeight, 1)
  const zoom = roundCoord(clampZoom(rawZoom))
  const x = roundCoord((stageWidth - contentWidth * zoom) / 2 - minX * zoom)
  const y = roundCoord((stageHeight - contentHeight * zoom) / 2 - minY * zoom)

  return { x, y, zoom }
}

/**
 * Zooms a viewport toward a focal point (in stage-local coordinates), keeping
 * the world point under the focal point stationary.
 */
export function zoomViewportAt(
  viewport: CanvasViewport,
  nextZoom: number,
  focalPoint: Point2D,
): CanvasViewport {
  const clamped = clampZoom(nextZoom)
  const worldX = (focalPoint.x - viewport.x) / viewport.zoom
  const worldY = (focalPoint.y - viewport.y) / viewport.zoom
  return {
    x: roundCoord(focalPoint.x - worldX * clamped),
    y: roundCoord(focalPoint.y - worldY * clamped),
    zoom: roundCoord(clamped),
  }
}

/**
 * Computes the new viewport for a two-finger pinch gesture given the initial
 * viewport, initial pinch distance/midpoint, and current pinch distance/midpoint
 * (midpoints in stage-local coordinates).
 */
export function computePinchViewport(
  startViewport: CanvasViewport,
  startDistance: number,
  startMidpoint: Point2D,
  currentDistance: number,
  currentMidpoint: Point2D,
): CanvasViewport {
  const safeStartDist = Math.max(1, startDistance)
  const nextZoom = clampZoom(startViewport.zoom * (currentDistance / safeStartDist))
  const worldMidX = (startMidpoint.x - startViewport.x) / startViewport.zoom
  const worldMidY = (startMidpoint.y - startViewport.y) / startViewport.zoom

  return {
    x: roundCoord(currentMidpoint.x - worldMidX * nextZoom),
    y: roundCoord(currentMidpoint.y - worldMidY * nextZoom),
    zoom: roundCoord(nextZoom),
  }
}

/**
 * Computes a smooth cubic Bézier path connecting the borders of `from` and `to`,
 * with automatic perpendicular curvature offset when multiple edges connect the same pair.
 */
export function computeEdgeCurve(
  from: CanvasNodeLayout,
  to: CanvasNodeLayout,
  parallelIndex = 0,
  parallelCount = 1,
): EdgeCurveGeometry {
  const fromCx = from.x + from.w / 2
  const fromCy = from.y + from.h / 2
  const toCx = to.x + to.w / 2
  const toCy = to.y + to.h / 2

  const dx = toCx - fromCx
  const dy = toCy - fromCy
  const horizontal = Math.abs(dx) >= Math.abs(dy)

  let start: Point2D
  let end: Point2D
  if (horizontal) {
    start = { x: dx >= 0 ? from.x + from.w : from.x, y: fromCy }
    end = { x: dx >= 0 ? to.x : to.x + to.w, y: toCy }
  } else {
    start = { x: fromCx, y: dy >= 0 ? from.y + from.h : from.y }
    end = { x: toCx, y: dy >= 0 ? to.y : to.y + to.h }
  }

  const segDx = end.x - start.x
  const segDy = end.y - start.y
  const length = Math.max(1, Math.hypot(segDx, segDy))
  const spreadOffset = (parallelIndex - (parallelCount - 1) / 2) * 34
  const perpX = (-segDy / length) * spreadOffset
  const perpY = (segDx / length) * spreadOffset

  let cp1: Point2D
  let cp2: Point2D
  if (horizontal) {
    const handle = Math.max(48, Math.min(160, Math.abs(segDx) * 0.45))
    const dir = segDx >= 0 ? 1 : -1
    cp1 = { x: start.x + dir * handle + perpX, y: start.y + perpY }
    cp2 = { x: end.x - dir * handle + perpX, y: end.y + perpY }
  } else {
    const handle = Math.max(48, Math.min(160, Math.abs(segDy) * 0.45))
    const dir = segDy >= 0 ? 1 : -1
    cp1 = { x: start.x + perpX, y: start.y + dir * handle + perpY }
    cp2 = { x: end.x + perpX, y: end.y - dir * handle + perpY }
  }

  // Midpoint on cubic Bézier at t = 0.5
  const label: Point2D = {
    x: roundCoord(0.125 * start.x + 0.375 * cp1.x + 0.375 * cp2.x + 0.125 * end.x),
    y: roundCoord(0.125 * start.y + 0.375 * cp1.y + 0.375 * cp2.y + 0.125 * end.y),
  }

  const path = `M ${roundCoord(start.x)} ${roundCoord(start.y)} C ${roundCoord(cp1.x)} ${roundCoord(cp1.y)}, ${roundCoord(cp2.x)} ${roundCoord(cp2.y)}, ${roundCoord(end.x)} ${roundCoord(end.y)}`

  return { path, start, end, label }
}
