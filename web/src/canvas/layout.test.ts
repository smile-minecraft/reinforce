import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import type { Edge, OutlineNode } from '../api/types.ts'
import {
  DEFAULT_NODE_HEIGHT,
  DEFAULT_NODE_WIDTH,
  collectOutlineCards,
  computeDeterministicLayout,
  computeEdgeCurve,
  computeFitViewport,
  computePinchViewport,
  parseCanvasViewSettings,
  resolveCanvasState,
  zoomViewportAt,
} from './layout.ts'

const OUTLINE_MULTI_SECTION: OutlineNode[] = [
  {
    type: 'section',
    heading: 'Phase changes',
    intro_markdown: null,
    children: [
      { type: 'card_ref', card_id: 'card-a' },
      { type: 'card_ref', card_id: 'card-b' },
    ],
  },
  {
    type: 'section',
    heading: 'Equations',
    intro_markdown: null,
    children: [
      { type: 'card_ref', card_id: 'card-c' },
      // Duplicate card reference in outline should appear once on canvas
      { type: 'card_ref', card_id: 'card-a' },
    ],
  },
]

const SAMPLE_EDGES: Edge[] = [
  {
    id: 'edge-1',
    edge_type: 'prerequisite',
    from: { kind: 'card', id: 'card-a' },
    to: { kind: 'card', id: 'card-b' },
  },
  {
    id: 'edge-2',
    edge_type: 'prerequisite',
    from: { kind: 'card', id: 'card-b' },
    to: { kind: 'card', id: 'card-c' },
  },
  {
    id: 'edge-3',
    edge_type: 'confusable_with',
    from: { kind: 'card', id: 'card-c' },
    to: { kind: 'card', id: 'card-a' },
  },
]

describe('computeDeterministicLayout', () => {
  it('produces identical output across repeated invocations and shuffled edge order', () => {
    const first = computeDeterministicLayout(OUTLINE_MULTI_SECTION, SAMPLE_EDGES)
    const second = computeDeterministicLayout(OUTLINE_MULTI_SECTION, SAMPLE_EDGES)
    const reversedEdges = [...SAMPLE_EDGES].reverse()
    const third = computeDeterministicLayout(OUTLINE_MULTI_SECTION, reversedEdges)

    assert.deepEqual(first, second)
    assert.deepEqual(first, third)
    assert.equal(first.length, 3)

    // Prerequisite chain card-a -> card-b -> card-c progresses strictly left to right.
    const byId = new Map(first.map((n) => [n.card_id, n]))
    assert.ok(byId.get('card-a')!.x < byId.get('card-b')!.x)
    assert.ok(byId.get('card-b')!.x < byId.get('card-c')!.x)
  })

  it('handles prerequisite cycles deterministically without infinite loops', () => {
    const cyclicEdges: Edge[] = [
      {
        id: 'e1',
        edge_type: 'prerequisite',
        from: { kind: 'card', id: 'card-a' },
        to: { kind: 'card', id: 'card-b' },
      },
      {
        id: 'e2',
        edge_type: 'prerequisite',
        from: { kind: 'card', id: 'card-b' },
        to: { kind: 'card', id: 'card-a' },
      },
    ]

    const run1 = computeDeterministicLayout(OUTLINE_MULTI_SECTION, cyclicEdges)
    const run2 = computeDeterministicLayout(OUTLINE_MULTI_SECTION, [...cyclicEdges].reverse())
    assert.deepEqual(run1, run2)
    assert.equal(run1.length, 3)
  })

  it('groups cards by outline sections when no prerequisite edges exist', () => {
    const layout = computeDeterministicLayout(OUTLINE_MULTI_SECTION, [])
    assert.equal(layout.length, 3)
    const byId = new Map(layout.map((n) => [n.card_id, n]))

    // Section 1: card-a and card-b on the same horizontal row.
    assert.equal(byId.get('card-a')!.y, byId.get('card-b')!.y)
    assert.ok(byId.get('card-a')!.x < byId.get('card-b')!.x)
    // Section 2: card-c placed on a lower row.
    assert.ok(byId.get('card-c')!.y > byId.get('card-a')!.y)
  })

  it('deduplicates repeated card_ids while preserving first section metadata', () => {
    const entries = collectOutlineCards(OUTLINE_MULTI_SECTION)
    assert.deepEqual(
      entries.map((e) => e.cardId),
      ['card-a', 'card-b', 'card-c'],
    )
    assert.equal(entries[0].sectionHeading, 'Phase changes')
    assert.equal(entries[2].sectionHeading, 'Equations')
  })
})

describe('parseCanvasViewSettings & resolveCanvasState', () => {
  it('parses valid saved canvas settings and rejects invalid bounds or non-finite numbers', () => {
    const valid = parseCanvasViewSettings({
      reading: { compact: true },
      canvas: {
        version: 1,
        nodes: [{ card_id: 'card-a', x: 10, y: 20, w: 320, h: 200 }],
        viewport: { x: -5, y: 12, zoom: 1.5 },
      },
    })
    assert.ok(valid)
    assert.equal(valid.nodes[0].x, 10)
    assert.equal(valid.viewport.zoom, 1.5)

    assert.equal(parseCanvasViewSettings({}), null)
    assert.equal(
      parseCanvasViewSettings({
        canvas: { version: 2, nodes: [], viewport: { x: 0, y: 0, zoom: 1 } },
      }),
      null,
    )
    assert.equal(
      parseCanvasViewSettings({
        canvas: { version: 1, nodes: [], viewport: { x: 0, y: 0, zoom: 4.5 } },
      }),
      null,
    )
    assert.equal(
      parseCanvasViewSettings({
        canvas: {
          version: 1,
          nodes: [{ card_id: 'card-a', x: Number.NaN, y: 0, w: 320, h: 200 }],
          viewport: { x: 0, y: 0, zoom: 1 },
        },
      }),
      null,
    )
  })

  it('preserves saved node positions, drops stale cards, and places newly added cards below', () => {
    const resolved = resolveCanvasState(OUTLINE_MULTI_SECTION, [], {
      canvas: {
        version: 1,
        nodes: [
          { card_id: 'card-a', x: 250, y: 120, w: 320, h: 200 },
          { card_id: 'stale-card', x: 999, y: 999, w: 320, h: 200 },
        ],
        viewport: { x: -20, y: 10, zoom: 0.9 },
      },
    })

    assert.equal(resolved.hasSavedLayout, true)
    assert.deepEqual(
      resolved.nodes.map((n) => n.card_id),
      ['card-a', 'card-b', 'card-c'],
    )
    assert.equal(resolved.nodes[0].x, 250)
    assert.equal(resolved.nodes[0].y, 120)
    // Newly added cards appear deterministically below card-a (120 + 200 = 320).
    assert.ok(resolved.nodes[1].y > 320)
    assert.ok(resolved.nodes[2].y > 320)
    assert.deepEqual(resolved.viewport, { x: -20, y: 10, zoom: 0.9 })
  })
})

describe('viewport & edge geometry helpers', () => {
  it('fits nodes within stage bounds and keeps focal point fixed while zooming/pinching', () => {
    const nodes = [
      { card_id: 'a', x: 48, y: 48, w: DEFAULT_NODE_WIDTH, h: DEFAULT_NODE_HEIGHT },
      { card_id: 'b', x: 464, y: 48, w: DEFAULT_NODE_WIDTH, h: DEFAULT_NODE_HEIGHT },
    ]
    const fitDesktop = computeFitViewport(nodes, 1200, 700)
    assert.equal(fitDesktop.zoom, 1)

    const fitMobile = computeFitViewport(nodes, 360, 560)
    assert.ok(fitMobile.zoom >= 0.1 && fitMobile.zoom < 1)

    const focal = { x: 300, y: 200 }
    const startVp = { x: 40, y: 20, zoom: 1 }
    const zoomed = zoomViewportAt(startVp, 2, focal)
    const worldBeforeX = (focal.x - startVp.x) / startVp.zoom
    const worldAfterX = (focal.x - zoomed.x) / zoomed.zoom
    assert.ok(Math.abs(worldBeforeX - worldAfterX) < 0.05)

    const pinched = computePinchViewport(startVp, 100, { x: 200, y: 150 }, 200, { x: 220, y: 170 })
    assert.equal(pinched.zoom, 2)
    assert.ok(Number.isFinite(pinched.x) && Number.isFinite(pinched.y))

    const curve = computeEdgeCurve(nodes[0], nodes[1])
    assert.ok(curve.path.startsWith('M '))
    assert.equal(curve.start.x, 48 + DEFAULT_NODE_WIDTH)
    assert.equal(curve.end.x, 464)
  })
})

describe('i18n locale parity', () => {
  function flattenKeys(obj: Record<string, unknown>, prefix = ''): string[] {
    const keys: string[] = []
    for (const [k, v] of Object.entries(obj)) {
      const next = prefix ? `${prefix}.${k}` : k
      if (v && typeof v === 'object' && !Array.isArray(v)) {
        keys.push(...flattenKeys(v as Record<string, unknown>, next))
      } else {
        keys.push(next)
      }
    }
    return keys.sort()
  }

  it('keeps en.json and zh-TW.json keys in sync', () => {
    const dir = dirname(fileURLToPath(import.meta.url))
    const en = JSON.parse(readFileSync(resolve(dir, '../i18n/locales/en.json'), 'utf8'))
    const zh = JSON.parse(readFileSync(resolve(dir, '../i18n/locales/zh-TW.json'), 'utf8'))
    assert.deepEqual(flattenKeys(en), flattenKeys(zh))
  })
})
