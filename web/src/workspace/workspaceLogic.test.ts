import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import type { QuestionCard, QuestionGroup, SubjectGroup } from '../api/types.ts'
import {
  buildGroupedQuestionSidebar,
  buildQuestionUnits,
  buildSubjectGroupTree,
  closeWorkspaceTab,
  findUnitIndexBySessionIndex,
  isDescendantOrSelf,
  mapConfidence5ToCore,
  openOrFocusTab,
  resolveGroupSubjects,
  type WorkspaceTab,
} from './workspaceLogic.ts'

function makeGroup(
  id: string,
  name: string,
  parent_id: string | null,
  subjects: string[],
  sort_order = 0,
): SubjectGroup {
  return {
    id,
    parent_id,
    name,
    sort_order,
    subjects,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  }
}

function makeQuestion(
  id: string,
  overrides: Partial<QuestionCard> = {},
): QuestionCard {
  return {
    id,
    exam_set_id: 'exam-1',
    question_group_id: null,
    question_no: 1,
    subject: 'physics',
    topics: ['thermo'],
    stem_blocks: [{ type: 'markdown', text: 'Stem' }],
    anchor_ids: [],
    question_type: 'single_choice',
    grading_mode: 'exact',
    options: [],
    answer_source_kind: 'official',
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('workspaceLogic: tabs', () => {
  const home: WorkspaceTab = { id: 'home', kind: 'home', path: '/', title: '首頁' }
  const noteA: WorkspaceTab = { id: 'note:a', kind: 'note', path: '/notes/a', title: '筆記 A' }
  const quiz1: WorkspaceTab = { id: 'session:1', kind: 'session', path: '/study/session/1', title: '複習 1' }

  it('opens new tab or focuses existing tab without duplicates', () => {
    const r1 = openOrFocusTab([home], noteA)
    assert.equal(r1.tabs.length, 2)
    assert.equal(r1.activeId, 'note:a')

    const r2 = openOrFocusTab(r1.tabs, { ...noteA, title: '筆記 A（更新）' })
    assert.equal(r2.tabs.length, 2)
    assert.equal(r2.tabs[1].title, '筆記 A（更新）')
    assert.equal(r2.activeId, 'note:a')
  })

  it('closes active tab and focuses adjacent tab, falling back to home if last', () => {
    const r1 = closeWorkspaceTab([home, noteA, quiz1], 'session:1', 'session:1', home)
    assert.equal(r1.tabs.length, 2)
    assert.equal(r1.activeId, 'note:a')
    assert.equal(r1.activePath, '/notes/a')

    const r2 = closeWorkspaceTab([noteA], 'note:a', 'note:a', home)
    assert.equal(r2.tabs.length, 1)
    assert.equal(r2.activeId, 'home')
  })
})

describe('workspaceLogic: nested subject groups', () => {
  const groups = [
    makeGroup('g-root', '自然科學', null, ['chemistry']),
    makeGroup('g-phys', '物理', 'g-root', ['physics']),
    makeGroup('g-thermo', '熱力學', 'g-phys', ['thermodynamics', 'physics']),
  ]

  it('builds nested tree and recursively resolves descendant subjects', () => {
    const tree = buildSubjectGroupTree(groups)
    assert.equal(tree.length, 1)
    assert.deepEqual(tree[0].resolvedSubjects, ['chemistry', 'physics', 'thermodynamics'])
    assert.deepEqual(resolveGroupSubjects(groups, 'g-phys'), ['physics', 'thermodynamics'])
  })

  it('detects cycles when moving a group under itself or a descendant', () => {
    assert.equal(isDescendantOrSelf(groups, 'g-root', 'g-root'), true)
    assert.equal(isDescendantOrSelf(groups, 'g-thermo', 'g-root'), true)
    assert.equal(isDescendantOrSelf(groups, 'g-root', 'g-thermo'), false)
    assert.equal(isDescendantOrSelf(groups, null, 'g-root'), false)
  })
})

describe('workspaceLogic: question groups and sidebar', () => {
  const q1 = makeQuestion('q1', { question_no: 1, question_group_id: null })
  const q2 = makeQuestion('q2', { question_no: 2, question_group_id: 'grp-1' })
  const q3 = makeQuestion('q3', { question_no: 3, question_group_id: 'grp-1' })
  const q4 = makeQuestion('q4', { question_no: 4, subject: 'biology', exam_set_id: 'exam-2' })

  const groups: Record<string, QuestionGroup> = {
    'grp-1': {
      id: 'grp-1',
      exam_set_id: 'exam-1',
      title: '實驗情境題組',
      shared_stem_blocks: [{ type: 'markdown', text: '共用題幹' }],
    },
  }

  const examSets = {
    'exam-1': { id: 'exam-1', title: '114 物理期中', exam_year: 2025, subject: 'physics' },
    'exam-2': { id: 'exam-2', title: '生物練習', exam_year: null, subject: 'biology' },
  }

  it('merges questions sharing question_group_id into a single QuestionUnit', () => {
    const units = buildQuestionUnits([q1, q2, q3, q4], groups, examSets)
    assert.equal(units.length, 3)
    assert.equal(units[0].items.length, 1)
    assert.equal(units[1].groupId, 'grp-1')
    assert.equal(units[1].items.length, 2)
    assert.deepEqual(
      units[1].items.map((i) => i.sessionIndex),
      [1, 2],
    )
    assert.equal(findUnitIndexBySessionIndex(units, 2), 1)
    assert.equal(findUnitIndexBySessionIndex(units, 3), 2)
  })

  it('groups units hierarchically by subject and exam year/title for right sidebar', () => {
    const units = buildQuestionUnits([q1, q2, q3, q4], groups, examSets)
    const buckets = buildGroupedQuestionSidebar(units, '綜合題目')
    assert.equal(buckets.length, 2)
    assert.equal(buckets[0].subject, 'physics')
    assert.equal(buckets[0].exams[0].label, '2025')
    assert.equal(buckets[0].exams[0].units.length, 2)
    assert.equal(buckets[1].subject, 'biology')
    assert.equal(buckets[1].exams[0].label, '生物練習')
  })
})

describe('workspaceLogic: 5-level confidence mapping', () => {
  it('maps 5 sketch buttons cleanly onto core 3-level confidence', () => {
    assert.deepEqual(mapConfidence5ToCore('certain'), { confidence: 'certain', impliedErrorTag: null })
    assert.deepEqual(mapConfidence5ToCore('fairly_sure'), { confidence: 'certain', impliedErrorTag: null })
    assert.deepEqual(mapConfidence5ToCore('hesitant'), { confidence: 'split', impliedErrorTag: null })
    assert.deepEqual(mapConfidence5ToCore('unfamiliar'), {
      confidence: 'split',
      impliedErrorTag: 'concept_gap',
    })
    assert.deepEqual(mapConfidence5ToCore('guessed'), { confidence: 'guessed', impliedErrorTag: null })
  })
})
