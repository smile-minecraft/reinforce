import type {
  AnswerRecord,
  CreateSessionRequest,
  GradedAnswer,
  GradingOutcome,
  QuestionCard,
  QuizAnswer,
  Score,
  SearchResult,
  SessionMode,
  SessionState,
} from '../types'
import {
  MOCK_ANCHORS,
  MOCK_EXAM_SETS,
  MOCK_GRAPH,
  MOCK_KNOWLEDGE_CARDS,
  MOCK_NOTES,
  MOCK_QUESTIONS,
  MOCK_RESOURCES,
  MOCK_WEAKNESS,
} from './data'

interface MockSession {
  id: string
  mode: SessionMode
  questions: QuestionCard[]
  state: SessionState
  deferGrading: boolean
  affectsScheduling: boolean
  pinRadius: number
  secondsPerQuestion: number | null
  totalTimeSecs: number | null
  questionStartedAt: number
  questionElapsedMs: number
  totalStartedAt: number
  totalElapsedMs: number
  answers: AnswerRecord[]
  graded: GradedAnswer[]
}

const sessions = new Map<string, MockSession>()
let sessionCounter = 1

function stripSecretFields(q: QuestionCard): QuestionCard {
  return {
    ...q,
    answer: undefined,
    accepted_answers: undefined,
    answer_basis: undefined,
    explanation_blocks: undefined,
    options: q.options.map((o) => ({ ...o, explanation: undefined })),
  }
}

function normalizeTerm(v: string): string {
  return v.trim().replace(/\s+/g, ' ').toLowerCase()
}

function gradeQuestion(
  q: QuestionCard,
  answer: QuizAnswer,
  pinRadius: number,
  affectsScheduling: boolean,
): GradingOutcome {
  if (q.grading_mode === 'voided') {
    return {
      is_correct: true,
      scoreable: false,
      voided: true,
      affects_scheduling: false,
      note: 'voided question',
    }
  }
  const sub = answer.submission
  if (sub.kind === 'timed_out') {
    return {
      is_correct: false,
      scoreable: true,
      voided: false,
      affects_scheduling: affectsScheduling,
      note: 'timed out',
    }
  }
  if (q.question_type === 'pin_locate' && sub.kind === 'pin_clicks') {
    const pins = q.stem_blocks.flatMap((b) => (b.type === 'interactive_pins' ? b.pins : []))
    const expectedNos = (q.answer ?? []).map((n) => Number(n))
    const targets = expectedNos.length > 0 ? expectedNos : pins.map((p) => p.no)
    const aspect = 800 / 500
    const allHit = targets.every((no) => {
      const pin = pins.find((p) => p.no === no)
      const click = sub.clicks.find((c) => c.no === no)
      if (!pin || !click) return false
      const dx = (click.point.x - pin.x) * aspect
      const dy = click.point.y - pin.y
      return Math.hypot(dx, dy) <= pinRadius
    })
    return {
      is_correct: allHit,
      scoreable: true,
      voided: false,
      affects_scheduling: affectsScheduling,
      note: allHit ? 'all pins hit' : 'pin missed',
    }
  }
  if (q.question_type === 'pin_identify' && sub.kind === 'text') {
    const pins = q.stem_blocks.flatMap((b) => (b.type === 'interactive_pins' ? b.pins : []))
    const norm = normalizeTerm(sub.value)
    const candidates = new Set<string>()
    for (const p of pins) {
      candidates.add(normalizeTerm(p.term_primary))
      if (p.term_secondary) candidates.add(normalizeTerm(p.term_secondary))
    }
    for (const grp of q.accepted_answers ?? []) {
      for (const item of grp) candidates.add(normalizeTerm(item))
    }
    const ok = candidates.has(norm)
    return {
      is_correct: ok,
      scoreable: true,
      voided: false,
      affects_scheduling: affectsScheduling,
      note: ok ? 'all pin terms matched' : 'pin term mismatch',
    }
  }
  if (sub.kind === 'text') {
    const norm = normalizeTerm(sub.value)
    const groups = [q.answer ?? [], ...(q.grading_mode === 'any_accepted' ? (q.accepted_answers ?? []) : [])]
    const ok = groups.some((g) => g.some((c) => normalizeTerm(c) === norm))
    return {
      is_correct: ok,
      scoreable: true,
      voided: false,
      affects_scheduling: affectsScheduling,
      note: ok ? 'text answer accepted' : 'text answer rejected',
    }
  }
  if (sub.kind === 'keys') {
    const canon = (arr: string[]) => [...arr].map((s) => s.trim()).filter(Boolean).sort().join('|')
    const submitted = canon(sub.values)
    const groups = [q.answer ?? [], ...(q.grading_mode === 'any_accepted' ? (q.accepted_answers ?? []) : [])]
    const ok = submitted.length > 0 && groups.some((g) => canon(g) === submitted)
    return {
      is_correct: ok,
      scoreable: true,
      voided: false,
      affects_scheduling: affectsScheduling,
      note: ok ? 'answer combination accepted' : 'answer combination rejected',
    }
  }
  return {
    is_correct: false,
    scoreable: true,
    voided: false,
    affects_scheduling: affectsScheduling,
    note: 'invalid submission',
  }
}

function computeScore(s: MockSession): Score {
  let correct = 0
  let scoreable = 0
  let voided = 0
  for (const g of s.graded) {
    if (g.outcome.voided) {
      voided += 1
      continue
    }
    if (g.outcome.scoreable) {
      scoreable += 1
      if (g.outcome.is_correct) correct += 1
    }
  }
  return {
    correct,
    scoreable,
    voided,
    ratio: scoreable === 0 ? 0 : correct / scoreable,
  }
}

function toWireState(state: SessionState): unknown {
  switch (state.status) {
    case 'not_started':
      return 'NotStarted'
    case 'submitted':
      return 'Submitted'
    case 'in_progress':
      return { InProgress: { index: state.index } }
    case 'paused':
      return { Paused: { index: state.index } }
    case 'expired':
      return { Expired: { index: state.index } }
  }
}

export async function handleMockRequest(
  path: string,
  init?: RequestInit,
): Promise<{ status: number; body: unknown }> {
  const method = (init?.method ?? 'GET').toUpperCase()
  const url = new URL(path, 'http://mock.local')
  const pathname = url.pathname

  if (pathname === '/api/meta') {
    return { status: 200, body: { version: '0.1.0', auth_mode: 'single' } }
  }
  if (pathname === '/api/auth/status') {
    return {
      status: 200,
      body: { auth_mode: 'single', needs_owner_setup: false, authenticated: true, role: 'owner' },
    }
  }
  if (pathname === '/api/auth/session') {
    return {
      status: 200,
      body: { user_id: '01a21714-0000-7000-8000-000000000001', role: 'owner' },
    }
  }

  // Notes
  if (pathname === '/api/notes') {
    const subject = url.searchParams.get('subject')
    const items = subject ? MOCK_NOTES.filter((n) => n.subject === subject) : MOCK_NOTES
    return { status: 200, body: items }
  }
  const noteMatch = pathname.match(/^\/api\/notes\/([^/]+)$/)
  if (noteMatch) {
    const found = MOCK_NOTES.find((n) => n.id === noteMatch[1])
    return found
      ? { status: 200, body: found }
      : { status: 404, body: { error: { code: 'not_found', message: 'note not found' } } }
  }

  // Knowledge cards
  if (pathname === '/api/knowledge_cards') {
    const subject = url.searchParams.get('subject')
    const items = subject
      ? MOCK_KNOWLEDGE_CARDS.filter((c) => c.subject === subject)
      : MOCK_KNOWLEDGE_CARDS
    return { status: 200, body: items }
  }
  const cardMatch = pathname.match(/^\/api\/knowledge_cards\/([^/]+)$/)
  if (cardMatch) {
    const found = MOCK_KNOWLEDGE_CARDS.find((c) => c.id === cardMatch[1])
    return found
      ? { status: 200, body: found }
      : { status: 404, body: { error: { code: 'not_found', message: 'knowledge card not found' } } }
  }

  // Questions
  if (pathname === '/api/questions') {
    const subject = url.searchParams.get('subject')
    const items = subject ? MOCK_QUESTIONS.filter((q) => q.subject === subject) : MOCK_QUESTIONS
    return { status: 200, body: items }
  }
  const qMatch = pathname.match(/^\/api\/questions\/([^/]+)$/)
  if (qMatch) {
    const found = MOCK_QUESTIONS.find((q) => q.id === qMatch[1])
    return found
      ? { status: 200, body: found }
      : { status: 404, body: { error: { code: 'not_found', message: 'question card not found' } } }
  }

  // Exam sets
  if (pathname === '/api/exam_sets') {
    return { status: 200, body: MOCK_EXAM_SETS }
  }
  const examMatch = pathname.match(/^\/api\/exam_sets\/([^/]+)$/)
  if (examMatch) {
    const found = MOCK_EXAM_SETS.find((e) => e.id === examMatch[1])
    return found
      ? { status: 200, body: found }
      : { status: 404, body: { error: { code: 'not_found', message: 'exam set not found' } } }
  }

  // Search
  if (pathname === '/api/search') {
    const q = (url.searchParams.get('q') ?? '').trim().toLowerCase()
    const results: SearchResult[] = []
    for (const c of MOCK_KNOWLEDGE_CARDS) {
      if (c.title.toLowerCase().includes(q) || c.topics.some((t) => t.toLowerCase().includes(q))) {
        results.push({ kind: 'knowledge_card', id: c.id, title: c.title })
      }
    }
    for (const n of MOCK_NOTES) {
      if (n.title.toLowerCase().includes(q) || n.subject.toLowerCase().includes(q)) {
        results.push({ kind: 'note', id: n.id, title: n.title })
      }
    }
    for (const item of MOCK_QUESTIONS) {
      const text = item.stem_blocks
        .map((b) => (b.type === 'markdown' ? b.text : ''))
        .join(' ')
        .toLowerCase()
      if (text.includes(q)) {
        results.push({ kind: 'question_card', id: item.id, title: item.stem_blocks })
      }
    }
    return { status: 200, body: results }
  }

  // Graph
  const graphMatch = pathname.match(/^\/api\/graph\/([^/]+)\/([^/]+)\/neighbors$/)
  if (graphMatch) {
    const key = `${graphMatch[1]}:${graphMatch[2]}`
    return { status: 200, body: MOCK_GRAPH[key] ?? [] }
  }

  // Resources & anchors
  const resMatch = pathname.match(/^\/api\/resources\/([^/]+)$/)
  if (resMatch) {
    const found = MOCK_RESOURCES.find((r) => r.id === resMatch[1])
    return found
      ? { status: 200, body: found }
      : { status: 404, body: { error: { code: 'not_found', message: 'resource not found' } } }
  }
  const anchorMatch = pathname.match(/^\/api\/anchors\/([^/]+)$/)
  if (anchorMatch) {
    const found = MOCK_ANCHORS.find((a) => a.id === anchorMatch[1])
    return found
      ? { status: 200, body: found }
      : { status: 404, body: { error: { code: 'not_found', message: 'anchor not found' } } }
  }

  // Study endpoints
  if (pathname === '/api/study/weakness') {
    return { status: 200, body: MOCK_WEAKNESS }
  }
  if (pathname === '/api/study/attempts') {
    return { status: 200, body: [] }
  }
  if (pathname === '/api/study/scheduling') {
    return { status: 200, body: null }
  }

  if (pathname === '/api/study/sessions' && method === 'POST') {
    const body = JSON.parse(String(init?.body ?? '{}')) as CreateSessionRequest
    let pool: QuestionCard[] = []
    if (body.mode === 'practical_exam') {
      pool = MOCK_QUESTIONS.filter((q) =>
        ['pin_identify', 'pin_locate', 'image_qa'].includes(q.question_type),
      )
    } else if (body.mode === 'full_exam') {
      pool = MOCK_QUESTIONS.filter((q) => q.exam_set_id === body.exam_set_id).sort(
        (a, b) => (a.question_no ?? 0) - (b.question_no ?? 0),
      )
    } else if (body.mode === 'topic_drill' && body.knowledge_card_ids?.length) {
      const wanted = new Set(body.knowledge_card_ids)
      pool = MOCK_QUESTIONS.filter((q) =>
        q.options.some((o) => o.linked_card_ids.some((id) => wanted.has(id))) ||
        q.stem_blocks.some(
          (b) =>
            b.type === 'interactive_pins' &&
            b.pins.some((p) => p.linked_card_id && wanted.has(p.linked_card_id)),
        ),
      )
      if (pool.length === 0) pool = MOCK_QUESTIONS.slice(0, 3)
    } else if (body.mode === 'weakness_drill') {
      pool = MOCK_QUESTIONS.slice(0, 3)
    } else {
      pool = MOCK_QUESTIONS.slice(0, body.options?.question_limit ?? 6)
    }

    const id = `mock-session-${sessionCounter++}`
    const now = Date.now()
    const initialState: SessionState =
      pool.length === 0 ? { status: 'submitted' } : { status: 'in_progress', index: 0 }
    const session: MockSession = {
      id,
      mode: body.mode,
      questions: pool,
      state: initialState,
      deferGrading: body.mode === 'full_exam',
      affectsScheduling: body.options?.affects_scheduling ?? true,
      pinRadius: body.options?.pin_radius ?? 0.08,
      secondsPerQuestion: body.mode === 'practical_exam' ? (body.options?.seconds_per_question ?? 30) : null,
      totalTimeSecs: body.mode === 'full_exam' ? (body.options?.total_time_secs ?? null) : null,
      questionStartedAt: now,
      questionElapsedMs: 0,
      totalStartedAt: now,
      totalElapsedMs: 0,
      answers: [],
      graded: [],
    }
    sessions.set(id, session)
    return {
      status: 200,
      body: {
        session_id: id,
        state: toWireState(session.state),
        question_count: session.questions.length,
      },
    }
  }

  const sessMatch = pathname.match(/^\/api\/study\/sessions\/([^/]+)(?:\/([^/]+))?$/)
  if (sessMatch) {
    const id = sessMatch[1]
    const action = sessMatch[2]
    const s = sessions.get(id)
    if (!s) {
      return { status: 404, body: { error: { code: 'not_found', message: 'quiz session not found' } } }
    }

    if (!action && method === 'GET') {
      const q =
        s.state.status === 'in_progress' ||
        s.state.status === 'paused' ||
        s.state.status === 'expired'
          ? s.questions[s.state.index]
          : null
      return {
        status: 200,
        body: {
          session_id: s.id,
          mode: s.mode,
          state: toWireState(s.state),
          question: q ? stripSecretFields(q) : null,
          question_count: s.questions.length,
        },
      }
    }

    if (action === 'pause' && method === 'POST') {
      if (s.state.status === 'in_progress') {
        const now = Date.now()
        s.questionElapsedMs += now - s.questionStartedAt
        s.totalElapsedMs += now - s.totalStartedAt
        s.state = { status: 'paused', index: s.state.index }
      }
      return { status: 200, body: { state: toWireState(s.state) } }
    }

    if (action === 'resume' && method === 'POST') {
      if (s.state.status === 'paused') {
        const now = Date.now()
        s.questionStartedAt = now
        s.totalStartedAt = now
        s.state = { status: 'in_progress', index: s.state.index }
      }
      return { status: 200, body: { state: toWireState(s.state) } }
    }

    if (action === 'tick' && method === 'POST') {
      if (s.state.status !== 'in_progress') {
        return { status: 200, body: 'None' }
      }
      const now = Date.now()
      const idx = s.state.index
      if (s.totalTimeSecs !== null) {
        const totalElapsed = s.totalElapsedMs + (now - s.totalStartedAt)
        if (totalElapsed >= s.totalTimeSecs * 1000) {
          s.state = { status: 'expired', index: idx }
          return { status: 200, body: { SessionExpired: { index: idx } } }
        }
      }
      if (s.secondsPerQuestion !== null) {
        const qElapsed = s.questionElapsedMs + (now - s.questionStartedAt)
        if (qElapsed >= s.secondsPerQuestion * 1000) {
          const question = s.questions[idx]
          const record: AnswerRecord = {
            index: idx,
            question_id: question.id,
            submission: { kind: 'timed_out' },
            confidence: 'guessed',
            duration_ms: s.secondsPerQuestion * 1000,
            created_at: new Date().toISOString(),
            timed_out: true,
          }
          const outcome = gradeQuestion(
            question,
            { submission: { kind: 'timed_out' }, confidence: 'guessed' },
            s.pinRadius,
            s.affectsScheduling,
          )
          const graded: GradedAnswer = { record, outcome }
          s.answers.push(record)
          s.graded.push(graded)
          const next = idx + 1
          s.state = next < s.questions.length ? { status: 'in_progress', index: next } : { status: 'submitted' }
          s.questionStartedAt = now
          s.questionElapsedMs = 0
          return { status: 200, body: { QuestionTimedOut: { index: idx, graded } } }
        }
      }
      return { status: 200, body: 'None' }
    }

    if (action === 'answers' && method === 'POST') {
      if (s.state.status !== 'in_progress') {
        return {
          status: 400,
          body: { error: { code: 'invalid_request', message: 'no question is in progress' } },
        }
      }
      const payload = JSON.parse(String(init?.body ?? '{}')) as QuizAnswer
      const idx = s.state.index
      const question = s.questions[idx]
      const now = Date.now()
      const durationMs = s.questionElapsedMs + (now - s.questionStartedAt)
      const record: AnswerRecord = {
        index: idx,
        question_id: question.id,
        submission: payload.submission,
        confidence: payload.confidence,
        duration_ms: durationMs,
        created_at: new Date().toISOString(),
        timed_out: false,
      }
      const outcome = gradeQuestion(question, payload, s.pinRadius, s.affectsScheduling)
      s.answers.push(record)
      if (!s.deferGrading) {
        s.graded.push({ record, outcome })
      }
      const next = idx + 1
      s.state = next < s.questions.length ? { status: 'in_progress', index: next } : { status: 'submitted' }
      s.questionStartedAt = now
      s.questionElapsedMs = 0
      return {
        status: 200,
        body: {
          state: toWireState(s.state),
          grading: s.deferGrading ? null : outcome,
        },
      }
    }

    if (action === 'finish' && method === 'POST') {
      if (s.deferGrading && s.graded.length < s.answers.length) {
        s.graded = s.answers.map((record) => {
          const question = s.questions[record.index]
          const outcome = gradeQuestion(
            question,
            { submission: record.submission, confidence: record.confidence },
            s.pinRadius,
            s.affectsScheduling,
          )
          return { record, outcome }
        })
      }
      s.state = { status: 'submitted' }
      return {
        status: 200,
        body: {
          attempts_written: s.graded.length,
          score: computeScore(s),
          graded: s.graded,
        },
      }
    }

    if (action === 'score' && method === 'GET') {
      return {
        status: 200,
        body: {
          score: computeScore(s),
          graded_count: s.graded.length,
        },
      }
    }
  }

  return { status: 404, body: { error: { code: 'not_found', message: `unknown mock path ${pathname}` } } }
}
