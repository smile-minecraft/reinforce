import type {
  Anchor,
  BatchOperation,
  OperationKind,
  Proposal,
  Resource,
  ValidationErrorItem,
  ValidationReport,
} from '../api/types'
import { parseProposalTarget } from '../api/types'

export function getProposalOperations(proposal: Proposal): BatchOperation[] {
  return Array.isArray(proposal.payload?.operations) ? proposal.payload.operations : []
}

export function getProposalSummaryTitle(proposal: Proposal): string {
  const ops = getProposalOperations(proposal)
  for (const op of ops) {
    switch (op.op) {
      case 'knowledge_card':
        if (op.data.title?.trim()) return op.data.title
        break
      case 'note':
        if (op.data.title?.trim()) return op.data.title
        break
      case 'exam_set':
        if (op.data.title?.trim()) return op.data.title
        break
      case 'question_group':
        if (op.data.title?.trim()) return op.data.title
        break
      case 'question_card': {
        const firstMd = op.data.stem_blocks?.find((b) => b.type === 'markdown')
        if (firstMd && firstMd.type === 'markdown' && firstMd.text.trim()) {
          const clean = firstMd.text.replace(/[#*_`>\n]+/g, ' ').trim()
          return clean.length > 72 ? `${clean.slice(0, 72)}…` : clean
        }
        break
      }
      case 'resource':
        if (op.data.title?.trim()) return op.data.title
        break
      default:
        break
    }
  }
  const target = parseProposalTarget(proposal.target)
  return `${target.kind} · ${target.id.slice(0, 8)}`
}

export function getProposalOpCounts(proposal: Proposal): Array<{ op: OperationKind; count: number }> {
  const counts = new Map<OperationKind, number>()
  for (const op of getProposalOperations(proposal)) {
    counts.set(op.op, (counts.get(op.op) ?? 0) + 1)
  }
  return Array.from(counts.entries()).map(([op, count]) => ({ op, count }))
}

export function proposalHasInteractivePins(proposal: Proposal): boolean {
  for (const op of getProposalOperations(proposal)) {
    if (
      op.op === 'knowledge_card' &&
      op.data.blocks.some((b) => b.type === 'interactive_pins')
    ) {
      return true
    }
    if (
      op.op === 'question_card' &&
      (op.data.stem_blocks.some((b) => b.type === 'interactive_pins') ||
        (op.data.explanation_blocks ?? []).some((b) => b.type === 'interactive_pins'))
    ) {
      return true
    }
  }
  return false
}

export function getOperationAnchorIds(op: BatchOperation): string[] {
  switch (op.op) {
    case 'knowledge_card':
      return op.data.anchor_ids ?? []
    case 'question_card':
      return op.data.anchor_ids ?? []
    case 'anchor':
      return op.data.id ? [op.data.id] : []
    default:
      return []
  }
}

export interface ProposalAnchorContext {
  anchorIds: string[]
  stagedAnchors: Map<string, Anchor>
  stagedResources: Map<string, Resource>
}

export function extractProposalAnchors(proposal: Proposal): ProposalAnchorContext {
  const ops = getProposalOperations(proposal)
  const stagedAnchors = new Map<string, Anchor>()
  const stagedResources = new Map<string, Resource>()
  const seen = new Set<string>()
  const anchorIds: string[] = []

  const pushAnchor = (id: string | undefined) => {
    if (!id || seen.has(id)) return
    seen.add(id)
    anchorIds.push(id)
  }

  for (const op of ops) {
    if (op.op === 'resource' && op.data.id) {
      stagedResources.set(op.data.id, op.data)
    }
    if (op.op === 'anchor' && op.data.id) {
      stagedAnchors.set(op.data.id, op.data)
      pushAnchor(op.data.id)
    }
  }

  for (const op of ops) {
    for (const aid of getOperationAnchorIds(op)) {
      pushAnchor(aid)
    }
  }

  return { anchorIds, stagedAnchors, stagedResources }
}

export function getOperationValidationErrors(
  report: ValidationReport | null,
  opIndex: number,
  tempRef?: string | null,
): ValidationErrorItem[] {
  if (!report) return []
  return report.errors.filter(
    (err) =>
      err.op_index === opIndex ||
      (tempRef != null && err.temp_ref != null && err.temp_ref === tempRef),
  )
}

export function getFieldValidationErrors(
  opErrors: ValidationErrorItem[],
  fieldNames: string[],
): ValidationErrorItem[] {
  return opErrors.filter((err) => {
    const normalized = err.field_path.replace(/^data\./, '')
    return fieldNames.some(
      (name) =>
        normalized === name ||
        normalized.startsWith(`${name}.`) ||
        normalized.startsWith(`${name}[`),
    )
  })
}

export function formatShortTimestamp(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date)
}
