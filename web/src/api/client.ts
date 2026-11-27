import type {
  AdjustPinsRequest,
  AdjustPinsResponse,
  Anchor,
  ApiMeta,
  AttemptDirection,
  AttemptLog,
  AuthSession,
  AuthStatus,
  CardScheduling,
  CreateSessionRequest,
  CreateSessionResponse,
  Edge,
  ExamSet,
  ExportRequest,
  ExportResult,
  FinishSessionResponse,
  GraphNode,
  KnowledgeCard,
  Note,
  Proposal,
  ProposalStatus,
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
  UpdateNoteViewSettingsRequest,
  UpdateOperationRequest,
  ValidationErrorItem,
  ValidationReport,
  WeaknessReport,
} from './types'
import { parseSessionState, parseTickOutcome } from './types'

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

export function extractValidationReport(error: unknown): ValidationReport | null {
  if (!(error instanceof ApiError) || error.status !== 422) return null
  const details = error.details
  if (typeof details !== 'object' || details === null || !('errors' in details)) return null
  const rawErrors = (details as { errors?: unknown }).errors
  if (!Array.isArray(rawErrors)) return null
  const errors: ValidationErrorItem[] = rawErrors.filter(
    (item): item is ValidationErrorItem =>
      typeof item === 'object' &&
      item !== null &&
      typeof (item as ValidationErrorItem).field_path === 'string' &&
      typeof (item as ValidationErrorItem).message === 'string',
  )
  return errors.length > 0 ? { errors } : null
}

let mockBlobUrlsCache: Record<string, string> | null = null

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

if (import.meta.env.DEV && isMockMode()) {
  void import('./mock/data').then((mod) => {
    mockBlobUrlsCache = mod.MOCK_BLOB_URLS
  })
}

export function resourceBlobUrl(resourceId: string): string {
  if (import.meta.env.DEV && isMockMode() && mockBlobUrlsCache?.[resourceId]) {
    return mockBlobUrlsCache[resourceId]
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

async function parseErrorResponse(response: Response): Promise<ApiError> {
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
  return new ApiError(response.status, code, message, details)
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  if (import.meta.env.DEV && isMockMode()) {
    const [{ handleMockRequest }, { MOCK_BLOB_URLS }] = await Promise.all([
      import('./mock/handler'),
      import('./mock/data'),
    ])
    mockBlobUrlsCache = MOCK_BLOB_URLS
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
    throw await parseErrorResponse(response)
  }

  if (response.status === 204) {
    return undefined as T
  }
  return (await response.json()) as T
}

function parseFilenameFromHeader(headerValue: string | null, fallback: string): string {
  if (!headerValue) return fallback
  const utf8Match = headerValue.match(/filename\*=UTF-8''([^;\s]+)/i)
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1])
    } catch {
      return utf8Match[1]
    }
  }
  const plainMatch = headerValue.match(/filename="?([^";\n]+)"?/i)
  return plainMatch?.[1]?.trim() || fallback
}

async function apiFetchBlob(
  path: string,
  init: RequestInit | undefined,
  fallbackFilename: string,
): Promise<ExportResult> {
  if (import.meta.env.DEV && isMockMode()) {
    const [{ handleMockBlobRequest }, { MOCK_BLOB_URLS }] = await Promise.all([
      import('./mock/handler'),
      import('./mock/data'),
    ])
    mockBlobUrlsCache = MOCK_BLOB_URLS
    const mockRes = await handleMockBlobRequest(path, init)
    if (mockRes.status >= 400) {
      const env = mockRes.errorBody as ErrorEnvelope
      throw new ApiError(
        mockRes.status,
        env?.error?.code ?? 'mock_error',
        env?.error?.message ?? 'Mock export failed',
        env?.error?.details,
      )
    }
    return {
      blob: mockRes.blob!,
      filename: mockRes.filename ?? fallbackFilename,
      contentType: mockRes.contentType ?? mockRes.blob!.type,
    }
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
    throw await parseErrorResponse(response)
  }

  const blob = await response.blob()
  const contentType = response.headers.get('Content-Type') ?? blob.type
  const filename = parseFilenameFromHeader(
    response.headers.get('Content-Disposition'),
    fallbackFilename,
  )
  return { blob, filename, contentType }
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
  listNoteEdges: (id: string) => apiFetch<Edge[]>(`/api/notes/${encodeURIComponent(id)}/edges`),
  updateNoteViewSettings: (id: string, body: UpdateNoteViewSettingsRequest) =>
    apiFetch<Note>(`/api/notes/${encodeURIComponent(id)}/view_settings`, {
      method: 'PUT',
      body: JSON.stringify(body),
    }),

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

  // Proposals & Triage
  listProposals: (status?: ProposalStatus) =>
    apiFetch<Proposal[]>(`/api/proposals${buildQuery({ status })}`),
  getProposal: (id: string) => apiFetch<Proposal>(`/api/proposals/${encodeURIComponent(id)}`),
  approveProposal: (id: string, body: { promote_verified: boolean }) =>
    apiFetch<Proposal>(`/api/proposals/${encodeURIComponent(id)}/approve`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  rejectProposal: (id: string, body: { review_note: string }) =>
    apiFetch<Proposal>(`/api/proposals/${encodeURIComponent(id)}/reject`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  adjustProposalPins: (id: string, body: AdjustPinsRequest) =>
    apiFetch<AdjustPinsResponse>(`/api/proposals/${encodeURIComponent(id)}/pins`, {
      method: 'PATCH',
      body: JSON.stringify(body),
    }),
  updateProposalOperation: (id: string, index: number, body: UpdateOperationRequest) =>
    apiFetch<Proposal>(
      `/api/proposals/${encodeURIComponent(id)}/operations/${encodeURIComponent(String(index))}`,
      {
        method: 'PATCH',
        body: JSON.stringify(body),
      },
    ),

  // Export
  exportContent: (body: ExportRequest) => {
    const ext = body.format === 'pdf' ? 'pdf' : 'zip'
    const scopeTag =
      body.scope.kind === 'note'
        ? 'note'
        : body.scope.kind === 'subject'
          ? body.scope.subject
          : 'weakness'
    const fallback = `reinforce-${scopeTag}-${body.preset ?? body.format}.${ext}`
    return apiFetchBlob(
      '/api/export',
      {
        method: 'POST',
        body: JSON.stringify(body),
      },
      fallback,
    )
  },

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
