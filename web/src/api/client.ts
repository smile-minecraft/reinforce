import type {
  Anchor,
  ApiMeta,
  AttemptDirection,
  AttemptLog,
  AuthSession,
  AuthStatus,
  CardScheduling,
  CreateSessionRequest,
  CreateSessionResponse,
  ExamSet,
  FinishSessionResponse,
  GraphNode,
  KnowledgeCard,
  Note,
  QuestionCard,
  QuizAnswer,
  RawSessionState,
  RawTickOutcome,
  Resource,
  ScoreResponse,
  SearchResult,
  SessionSnapshot,
  SessionState,
  SubmitAnswerResponse,
  TickOutcome,
  WeaknessReport,
} from './types'
import { parseSessionState, parseTickOutcome } from './types'
import { MOCK_BLOB_URLS } from './mock/data'

export class ApiError extends Error {
  status: number
  code: string
  details?: unknown

  constructor(status: number, code: string, message: string, details?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.details = details
  }
}

export function isMockMode(): boolean {
  if (!import.meta.env.DEV) return false
  if (typeof window === 'undefined') return false
  const params = new URLSearchParams(window.location.search)
  if (params.get('mock') === '1') {
    window.sessionStorage.setItem('reinforce:mock', '1')
    return true
  }
  if (params.get('mock') === '0') {
    window.sessionStorage.removeItem('reinforce:mock')
    return false
  }
  return window.sessionStorage.getItem('reinforce:mock') === '1'
}

export function resourceBlobUrl(resourceId: string): string {
  if (isMockMode() && MOCK_BLOB_URLS[resourceId]) {
    return MOCK_BLOB_URLS[resourceId]
  }
  return `/api/resources/${encodeURIComponent(resourceId)}/blob`
}

interface ErrorEnvelope {
  error?: {
    code?: string
    message?: string
    details?: unknown
  }
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  if (import.meta.env.DEV && isMockMode()) {
    const { handleMockRequest } = await import('./mock/handler')
    const mockRes = await handleMockRequest(path, init)
    if (mockRes.status >= 400) {
      const env = mockRes.body as ErrorEnvelope
      throw new ApiError(
        mockRes.status,
        env?.error?.code ?? 'mock_error',
        env?.error?.message ?? 'Mock request failed',
        env?.error?.details,
      )
    }
    return mockRes.body as T
  }

  const headers = new Headers(init?.headers)
  if (init?.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(path, {
    ...init,
    headers,
    credentials: 'same-origin',
  })

  if (!response.ok) {
    let code = `http_${response.status}`
    let message = response.statusText || 'Request failed'
    let details: unknown
    try {
      const parsed = (await response.json()) as ErrorEnvelope
      if (parsed?.error) {
        code = parsed.error.code ?? code
        message = parsed.error.message ?? message
        details = parsed.error.details
      }
    } catch {
      // Fall back to HTTP status text when response is not JSON.
    }
    throw new ApiError(response.status, code, message, details)
  }

  if (response.status === 204) {
    return undefined as T
  }
  return (await response.json()) as T
}

function buildQuery(params: Record<string, string | number | undefined>): string {
  const qs = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== '') qs.set(k, String(v))
  }
  const str = qs.toString()
  return str ? `?${str}` : ''
}

export const api = {
  // Meta & Auth
  getMeta: () => apiFetch<ApiMeta>('/api/meta'),
  getAuthStatus: () => apiFetch<AuthStatus>('/api/auth/status'),
  getAuthSession: () => apiFetch<AuthSession>('/api/auth/session'),
  login: (body: { username: string; password: string }) =>
    apiFetch<AuthSession>('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  register: (body: { invite_code: string; username: string; password: string }) =>
    apiFetch<{ user_id: string }>('/api/auth/register', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  logout: () => apiFetch<{ ok: boolean }>('/api/auth/logout', { method: 'POST' }),

  // Content
  listNotes: (params?: { subject?: string; status?: string; limit?: number }) =>
    apiFetch<Note[]>(`/api/notes${buildQuery(params ?? {})}`),
  getNote: (id: string) => apiFetch<Note>(`/api/notes/${encodeURIComponent(id)}`),

  listKnowledgeCards: (params?: { subject?: string; status?: string; limit?: number }) =>
    apiFetch<KnowledgeCard[]>(`/api/knowledge_cards${buildQuery(params ?? {})}`),
  getKnowledgeCard: (id: string) =>
    apiFetch<KnowledgeCard>(`/api/knowledge_cards/${encodeURIComponent(id)}`),

  listQuestions: (params?: { subject?: string; status?: string; limit?: number }) =>
    apiFetch<QuestionCard[]>(`/api/questions${buildQuery(params ?? {})}`),
  getQuestion: (id: string) => apiFetch<QuestionCard>(`/api/questions/${encodeURIComponent(id)}`),

  listExamSets: (params?: { subject?: string; limit?: number }) =>
    apiFetch<ExamSet[]>(`/api/exam_sets${buildQuery(params ?? {})}`),
  getExamSet: (id: string) => apiFetch<ExamSet>(`/api/exam_sets/${encodeURIComponent(id)}`),

  search: (q: string, limit?: number) =>
    apiFetch<SearchResult[]>(`/api/search${buildQuery({ q, limit })}`),

  graphNeighbors: (kind: 'card' | 'question' | 'option', id: string, depth = 1) =>
    apiFetch<GraphNode[]>(
      `/api/graph/${encodeURIComponent(kind)}/${encodeURIComponent(id)}/neighbors${buildQuery({ depth })}`,
    ),

  getResource: (id: string) => apiFetch<Resource>(`/api/resources/${encodeURIComponent(id)}`),
  getAnchor: (id: string) => apiFetch<Anchor>(`/api/anchors/${encodeURIComponent(id)}`),

  // Study
  createSession: async (request: CreateSessionRequest): Promise<CreateSessionResponse> => {
    const raw = await apiFetch<{
      session_id: string
      state: RawSessionState
      question_count: number
    }>('/api/study/sessions', {
      method: 'POST',
      body: JSON.stringify(request),
    })
    return {
      session_id: raw.session_id,
      state: parseSessionState(raw.state),
      question_count: raw.question_count,
    }
  },

  getSession: async (id: string): Promise<SessionSnapshot> => {
    const raw = await apiFetch<{
      session_id: string
      mode: SessionSnapshot['mode']
      state: RawSessionState
      question: QuestionCard | null
      question_count: number
    }>(`/api/study/sessions/${encodeURIComponent(id)}`)
    return {
      session_id: raw.session_id,
      mode: raw.mode,
      state: parseSessionState(raw.state),
      question: raw.question,
      question_count: raw.question_count,
    }
  },

  startSession: async (id: string): Promise<{ state: SessionState }> => {
    const raw = await apiFetch<{ state: RawSessionState }>(
      `/api/study/sessions/${encodeURIComponent(id)}/start`,
      { method: 'POST' },
    )
    return { state: parseSessionState(raw.state) }
  },

  pauseSession: async (id: string): Promise<{ state: SessionState }> => {
    const raw = await apiFetch<{ state: RawSessionState }>(
      `/api/study/sessions/${encodeURIComponent(id)}/pause`,
      { method: 'POST' },
    )
    return { state: parseSessionState(raw.state) }
  },

  resumeSession: async (id: string): Promise<{ state: SessionState }> => {
    const raw = await apiFetch<{ state: RawSessionState }>(
      `/api/study/sessions/${encodeURIComponent(id)}/resume`,
      { method: 'POST' },
    )
    return { state: parseSessionState(raw.state) }
  },

  tickSession: async (id: string): Promise<TickOutcome> => {
    const raw = await apiFetch<RawTickOutcome>(
      `/api/study/sessions/${encodeURIComponent(id)}/tick`,
      { method: 'POST' },
    )
    return parseTickOutcome(raw)
  },

  submitAnswer: async (id: string, answer: QuizAnswer): Promise<SubmitAnswerResponse> => {
    const raw = await apiFetch<{
      state: RawSessionState
      grading: SubmitAnswerResponse['grading']
      revealed_question?: QuestionCard | null
    }>(`/api/study/sessions/${encodeURIComponent(id)}/answers`, {
      method: 'POST',
      body: JSON.stringify(answer),
    })
    return {
      state: parseSessionState(raw.state),
      grading: raw.grading,
      revealed_question: raw.revealed_question,
    }
  },

  finishSession: (id: string) =>
    apiFetch<FinishSessionResponse>(`/api/study/sessions/${encodeURIComponent(id)}/finish`, {
      method: 'POST',
    }),

  getSessionScore: (id: string) =>
    apiFetch<ScoreResponse>(`/api/study/sessions/${encodeURIComponent(id)}/score`),

  listAttempts: (params?: { card_id?: string; direction?: AttemptDirection }) =>
    apiFetch<AttemptLog[]>(`/api/study/attempts${buildQuery(params ?? {})}`),

  getScheduling: (cardId: string, direction?: AttemptDirection) =>
    apiFetch<CardScheduling | null>(
      `/api/study/scheduling${buildQuery({ card_id: cardId, direction })}`,
    ),

  getWeaknessReport: (limit = 20) =>
    apiFetch<WeaknessReport>(`/api/study/weakness${buildQuery({ limit })}`),
}
