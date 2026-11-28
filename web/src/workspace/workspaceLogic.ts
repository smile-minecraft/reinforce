import type {
  Confidence,
  ExamSetMeta,
  QuestionCard,
  QuestionGroup,
  SubjectGroup,
} from '../api/types'

// ---------------------------------------------------------------------------
// 1. Multi-tab workspace state
// ---------------------------------------------------------------------------

export type WorkspaceTabKind =
  | 'home'
  | 'note'
  | 'card'
  | 'study'
  | 'session'
  | 'analytics'
  | 'triage'
  | 'export'
  | 'settings'
  | 'login'

export interface WorkspaceTab {
  id: string
  kind: WorkspaceTabKind
  path: string
  title: string
}

export function openOrFocusTab(
  tabs: WorkspaceTab[],
  incoming: WorkspaceTab,
): { tabs: WorkspaceTab[]; activeId: string } {
  const byId = tabs.find((t) => t.id === incoming.id)
  if (byId) {
    const nextTabs = tabs.map((t) =>
      t.id === incoming.id
        ? { ...t, path: incoming.path, title: incoming.title || t.title }
        : t,
    )
    return { tabs: nextTabs, activeId: incoming.id }
  }
  const byPath = tabs.find((t) => t.path === incoming.path)
  if (byPath) {
    const nextTabs = tabs.map((t) =>
      t.id === byPath.id ? { ...t, title: incoming.title || t.title } : t,
    )
    return { tabs: nextTabs, activeId: byPath.id }
  }
  return { tabs: [...tabs, incoming], activeId: incoming.id }
}

export function closeWorkspaceTab(
  tabs: WorkspaceTab[],
  activeId: string,
  targetId: string,
  fallbackTab: WorkspaceTab,
): { tabs: WorkspaceTab[]; activeId: string; activePath: string } {
  const idx = tabs.findIndex((t) => t.id === targetId)
  if (idx === -1) {
    const active = tabs.find((t) => t.id === activeId) ?? tabs[0] ?? fallbackTab
    return { tabs: tabs.length > 0 ? tabs : [fallbackTab], activeId: active.id, activePath: active.path }
  }
  const remaining = tabs.filter((t) => t.id !== targetId)
  if (remaining.length === 0) {
    return { tabs: [fallbackTab], activeId: fallbackTab.id, activePath: fallbackTab.path }
  }
  if (activeId !== targetId) {
    const active = remaining.find((t) => t.id === activeId) ?? remaining[0]
    return { tabs: remaining, activeId: active.id, activePath: active.path }
  }
  const nextActive = remaining[Math.max(0, idx - 1)] ?? remaining[0]
  return { tabs: remaining, activeId: nextActive.id, activePath: nextActive.path }
}

export function updateWorkspaceTabTitle(
  tabs: WorkspaceTab[],
  tabId: string,
  title: string,
): WorkspaceTab[] {
  const trimmed = title.trim()
  if (!trimmed) return tabs
  let changed = false
  const next = tabs.map((t) => {
    if (t.id === tabId && t.title !== trimmed) {
      changed = true
      return { ...t, title: trimmed }
    }
    return t
  })
  return changed ? next : tabs
}

// ---------------------------------------------------------------------------
// 2. Nested Subject Groups tree helpers
// ---------------------------------------------------------------------------

export interface SubjectGroupNode {
  group: SubjectGroup
  depth: number
  children: SubjectGroupNode[]
  resolvedSubjects: string[]
}

export function resolveGroupSubjects(
  groups: SubjectGroup[],
  groupId: string,
): string[] {
  const byId = new Map<string, SubjectGroup>()
  const childrenByParent = new Map<string, SubjectGroup[]>()
  for (const g of groups) {
    byId.set(g.id, g)
    if (g.parent_id) {
      const list = childrenByParent.get(g.parent_id) ?? []
      list.push(g)
      childrenByParent.set(g.parent_id, list)
    }
  }
  const result = new Set<string>()
  const visited = new Set<string>()
  const stack: string[] = [groupId]
  while (stack.length > 0) {
    const currentId = stack.pop()!
    if (visited.has(currentId)) continue
    visited.add(currentId)
    const group = byId.get(currentId)
    if (!group) continue
    for (const s of group.subjects) result.add(s)
    for (const child of childrenByParent.get(currentId) ?? []) {
      stack.push(child.id)
    }
  }
  return Array.from(result).sort()
}

export function isDescendantOrSelf(
  groups: SubjectGroup[],
  candidateParentId: string | null,
  nodeId: string,
): boolean {
  if (!candidateParentId) return false
  if (candidateParentId === nodeId) return true
  const parentOf = new Map<string, string | null>()
  for (const g of groups) parentOf.set(g.id, g.parent_id)
  const visited = new Set<string>()
  let cursor: string | null = candidateParentId
  while (cursor) {
    if (cursor === nodeId) return true
    if (visited.has(cursor)) break
    visited.add(cursor)
    cursor = parentOf.get(cursor) ?? null
  }
  return false
}

export function buildSubjectGroupTree(groups: SubjectGroup[]): SubjectGroupNode[] {
  const sorted = [...groups].sort(
    (a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name) || a.id.localeCompare(b.id),
  )
  const byId = new Map<string, SubjectGroup>()
  const childrenMap = new Map<string | null, SubjectGroup[]>()
  for (const g of sorted) {
    byId.set(g.id, g)
  }
  for (const g of sorted) {
    const parentKey = g.parent_id && byId.has(g.parent_id) ? g.parent_id : null
    const list = childrenMap.get(parentKey) ?? []
    list.push(g)
    childrenMap.set(parentKey, list)
  }

  function build(parentId: string | null, depth: number, trail: Set<string>): SubjectGroupNode[] {
    const list = childrenMap.get(parentId) ?? []
    const nodes: SubjectGroupNode[] = []
    for (const group of list) {
      if (trail.has(group.id)) continue
      const nextTrail = new Set(trail)
      nextTrail.add(group.id)
      const children = build(group.id, depth + 1, nextTrail)
      const resolved = new Set<string>(group.subjects)
      for (const child of children) {
        for (const s of child.resolvedSubjects) resolved.add(s)
      }
      nodes.push({
        group,
        depth,
        children,
        resolvedSubjects: Array.from(resolved).sort(),
      })
    }
    return nodes
  }

  return build(null, 0, new Set())
}

export function flattenSubjectGroupTree(nodes: SubjectGroupNode[]): SubjectGroupNode[] {
  const out: SubjectGroupNode[] = []
  function walk(list: SubjectGroupNode[]) {
    for (const node of list) {
      out.push(node)
      walk(node.children)
    }
  }
  walk(nodes)
  return out
}

// ---------------------------------------------------------------------------
// 3. QuestionGroup unit grouping & right-sidebar grouped tree
// ---------------------------------------------------------------------------

export interface QuestionUnitItem {
  sessionIndex: number
  question: QuestionCard
}

export interface QuestionUnit {
  unitId: string
  unitIndex: number
  groupId: string | null
  group: QuestionGroup | null
  subject: string
  examSetId: string | null
  examYear: number | null
  examTitle: string | null
  items: QuestionUnitItem[]
}

export function buildQuestionUnits(
  questions: QuestionCard[],
  questionGroups: Record<string, QuestionGroup> = {},
  examSets: Record<string, ExamSetMeta> = {},
  indexOffset = 0,
): QuestionUnit[] {
  const units: QuestionUnit[] = []
  const groupUnitMap = new Map<string, QuestionUnit>()

  questions.forEach((question, idx) => {
    const sessionIndex = indexOffset + idx
    const groupId = question.question_group_id
    if (groupId && groupUnitMap.has(groupId)) {
      groupUnitMap.get(groupId)!.items.push({ sessionIndex, question })
      return
    }

    const examMeta = question.exam_set_id ? examSets[question.exam_set_id] : undefined
    const unit: QuestionUnit = {
      unitId: groupId ? `group:${groupId}` : `q:${question.id}:${sessionIndex}`,
      unitIndex: units.length,
      groupId: groupId ?? null,
      group: groupId ? (questionGroups[groupId] ?? null) : null,
      subject: question.subject,
      examSetId: question.exam_set_id,
      examYear: examMeta?.exam_year ?? null,
      examTitle: examMeta?.title ?? null,
      items: [{ sessionIndex, question }],
    }
    units.push(unit)
    if (groupId) {
      groupUnitMap.set(groupId, unit)
    }
  })

  return units
}

export function findUnitIndexBySessionIndex(
  units: QuestionUnit[],
  sessionIndex: number,
): number {
  const found = units.findIndex((u) => u.items.some((item) => item.sessionIndex === sessionIndex))
  return found >= 0 ? found : 0
}

export interface SidebarExamBucket {
  key: string
  label: string
  units: QuestionUnit[]
}

export interface SidebarSubjectBucket {
  subject: string
  exams: SidebarExamBucket[]
}

export function buildGroupedQuestionSidebar(
  units: QuestionUnit[],
  ungroupedExamLabel: string,
): SidebarSubjectBucket[] {
  const subjectMap = new Map<string, Map<string, SidebarExamBucket>>()
  const subjectOrder: string[] = []

  for (const unit of units) {
    if (!subjectMap.has(unit.subject)) {
      subjectMap.set(unit.subject, new Map())
      subjectOrder.push(unit.subject)
    }
    const examsMap = subjectMap.get(unit.subject)!
    const examKey = unit.examSetId ?? (unit.examYear ? `year:${unit.examYear}` : '__general__')
    const examLabel = unit.examYear
      ? `${unit.examYear}`
      : unit.examTitle ?? ungroupedExamLabel
    if (!examsMap.has(examKey)) {
      examsMap.set(examKey, { key: examKey, label: examLabel, units: [] })
    }
    examsMap.get(examKey)!.units.push(unit)
  }

  return subjectOrder.map((subject) => ({
    subject,
    exams: Array.from(subjectMap.get(subject)!.values()),
  }))
}

// ---------------------------------------------------------------------------
// 4. Five-level confidence selector (Sketch 1 bottom bar) -> Core Confidence
// ---------------------------------------------------------------------------

export type ConfidenceLevel5 = 'certain' | 'fairly_sure' | 'hesitant' | 'unfamiliar' | 'guessed'

export const CONFIDENCE_LEVELS_5: ConfidenceLevel5[] = [
  'certain',
  'fairly_sure',
  'hesitant',
  'unfamiliar',
  'guessed',
]

export function mapConfidence5ToCore(level: ConfidenceLevel5): {
  confidence: Confidence
  impliedErrorTag: string | null
} {
  switch (level) {
    case 'certain':
      return { confidence: 'certain', impliedErrorTag: null }
    case 'fairly_sure':
      return { confidence: 'certain', impliedErrorTag: null }
    case 'hesitant':
      return { confidence: 'split', impliedErrorTag: null }
    case 'unfamiliar':
      return { confidence: 'split', impliedErrorTag: 'concept_gap' }
    case 'guessed':
      return { confidence: 'guessed', impliedErrorTag: null }
  }
}

export function inferConfidence5FromCore(
  confidence: Confidence,
  errorTags: string[] = [],
): ConfidenceLevel5 {
  if (confidence === 'guessed') return 'guessed'
  if (confidence === 'split') {
    return errorTags.includes('concept_gap') ? 'unfamiliar' : 'hesitant'
  }
  return 'certain'
}
