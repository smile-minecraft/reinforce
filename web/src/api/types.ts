// Domain & API types mirroring crates/reinforce-core and crates/reinforce-server.

export type ResourceKind = 'pdf' | 'image' | 'audio' | 'video' | 'transcript' | 'web'
export type Sensitivity = 'normal' | 'restricted'
export type ExportPolicy = 'allow' | 'redact' | 'owner_only'
export type AnchorKind = 'pdf_page' | 'rect' | 'media_span'
export type CardStatus = 'draft' | 'staging' | 'published' | 'archived'
export type Verification = 'unchecked' | 'verified'
export type DiagramSyntax = 'mermaid' | 'tikz' | 'svg'
export type SourceKind = 'past_exam' | 'question_bank' | 'original' | 'custom'
export type QuestionType =
  | 'single_choice'
  | 'multiple_choice'
  | 'short_answer'
  | 'vocab'
  | 'image_qa'
  | 'image_occlusion'
  | 'pin_identify'
  | 'pin_locate'
export type GradingMode = 'exact' | 'any_accepted' | 'voided'
export type AnswerSourceKind =
  | 'official'
  | 'peer_note'
  | 'duplicate_question'
  | 'inferred'
  | 'original_authored'
  | 'unanswered'
export type EdgeType = 'tests' | 'confusable_with' | 'prerequisite' | 'variant_of'
export type AttemptDirection = 'default' | 'forward' | 'reverse'
export type SessionMode =
  | 'daily_due'
  | 'full_exam'
  | 'topic_drill'
  | 'practical_exam'
  | 'weakness_drill'
export type Confidence = 'certain' | 'split' | 'guessed'
export type SchedulingState = 'new' | 'learning' | 'review' | 'relearning'
export type AuthMode = 'single' | 'invite'
export type UserRole = 'owner' | 'learner'

export interface NormalizedRect {
  x: number
  y: number
  w: number
  h: number
}

export interface NormalizedPoint {
  x: number
  y: number
}

export interface MediaSpan {
  start_ms: number
  end_ms: number
}

export interface Resource {
  id: string
  kind: ResourceKind
  title: string
  sha256: string
  byte_size: number
  page_count: number | null
  width?: number | null
  height?: number | null
  license: string | null
  attribution_url: string | null
  attribution_notice: string | null
  sensitivity: Sensitivity
  export_policy: ExportPolicy
  created_at: string
}

export interface Anchor {
  id: string
  resource_id: string
  anchor_kind: AnchorKind
  pdf_page: number | null
  printed_page: number | null
  legend_pdf_page: number | null
  rect: NormalizedRect | null
  media_span: MediaSpan | null
}

export interface OcclusionMask {
  group_id: string
  rect: NormalizedRect
}

export interface DiagramNodeBinding {
  node_id: string
  card_id: string
}

export interface Pin {
  no: number
  x: number
  y: number
  label_x: number
  label_y: number
  term_primary: string
  term_secondary: string | null
  detail: string | null
  hint: string | null
  linked_card_id: string | null
}

// Discriminated union for the 5 first-class content blocks.
export type ContentBlock =
  | {
      type: 'markdown'
      text: string
    }
  | {
      type: 'math'
      latex: string
      display: boolean
    }
  | {
      type: 'diagram'
      source: string
      syntax: DiagramSyntax
      svg_cache: string | null
      node_bindings: DiagramNodeBinding[]
    }
  | {
      type: 'image_occlusion'
      resource_id: string
      masks: OcclusionMask[]
    }
  | {
      type: 'interactive_pins'
      resource_id: string
      overlay_resource_id: string | null
      pins: Pin[]
    }

export interface KnowledgeCard {
  id: string
  subject: string
  topics: string[]
  title: string
  blocks: ContentBlock[]
  anchor_ids: string[]
  status: CardStatus
  verification: Verification
  unsure_count: number
  metadata: Record<string, unknown>
  created_at: string
  updated_at: string
}

export type OutlineNode =
  | {
      type: 'section'
      heading: string
      intro_markdown: string | null
      children: OutlineNode[]
    }
  | {
      type: 'card_ref'
      card_id: string
    }

export interface Note {
  id: string
  subject: string
  title: string
  outline: OutlineNode[]
  view_settings: Record<string, unknown>
  status: CardStatus
  created_at: string
  updated_at: string
}

export interface ExamSet {
  id: string
  subject: string
  title: string
  source_kind: SourceKind
  exam_year: number | null
  source_resource_id: string | null
  expected_questions: number | null
  metadata: Record<string, unknown>
  is_complete: boolean
}

export interface QuestionOption {
  key: string
  content: string
  explanation?: string | null
  linked_card_ids: string[]
}

export interface QuestionCard {
  id: string
  exam_set_id: string | null
  question_group_id: string | null
  question_no: number | null
  subject: string
  topics: string[]
  stem_blocks: ContentBlock[]
  explanation_blocks?: ContentBlock[]
  anchor_ids: string[]
  question_type: QuestionType
  grading_mode: GradingMode
  options: QuestionOption[]
  answer?: string[]
  accepted_answers?: string[][]
  answer_source_kind: AnswerSourceKind
  answer_basis?: string | null
  duplicate_question_id?: string | null
  status: CardStatus
  verification: Verification
  unsure_count: number
  metadata: Record<string, unknown>
  created_at: string
  updated_at: string
}

export type EdgeEndpoint =
  | { kind: 'card'; id: string }
  | { kind: 'question'; id: string }
  | { kind: 'option'; id: string; key: string }

export interface GraphNode {
  endpoint: EdgeEndpoint
  depth: number
}

export type SearchResult =
  | { kind: 'knowledge_card'; id: string; title: string }
  | { kind: 'question_card'; id: string; title: ContentBlock[] }
  | { kind: 'note'; id: string; title: string }

export interface PinClick {
  no: number
  point: NormalizedPoint
}

// Discriminated union matching crates/reinforce-core/src/quiz/grade.rs
export type AnswerSubmission =
  | { kind: 'keys'; values: string[] }
  | { kind: 'text'; value: string }
  | { kind: 'pin_clicks'; clicks: PinClick[] }
  | { kind: 'timed_out' }

export interface QuizAnswer {
  submission: AnswerSubmission
  confidence: Confidence
  error_tags?: string[]
}

export interface GradingOutcome {
  is_correct: boolean
  scoreable: boolean
  voided: boolean
  affects_scheduling: boolean
  note: string
}

export interface AnswerRecord {
  index: number
  question_id: string
  submission: AnswerSubmission
  confidence: Confidence
  duration_ms: number
  created_at: string
  timed_out: boolean
}

export interface GradedAnswer {
  record: AnswerRecord
  outcome: GradingOutcome
}

export interface Score {
  correct: number
  scoreable: number
  voided: number
  ratio: number
}

// Wire representation from serde's externally-tagged Rust enum SessionState
export type RawSessionState =
  | 'NotStarted'
  | 'Submitted'
  | { InProgress: { index: number } }
  | { Paused: { index: number } }
  | { Expired: { index: number } }
  | SessionState

// Normalized discriminated union used across the frontend
export type SessionState =
  | { status: 'not_started' }
  | { status: 'in_progress'; index: number }
  | { status: 'paused'; index: number }
  | { status: 'expired'; index: number }
  | { status: 'submitted' }

export function parseSessionState(raw: RawSessionState): SessionState {
  if (raw === 'NotStarted') return { status: 'not_started' }
  if (raw === 'Submitted') return { status: 'submitted' }
  if (typeof raw === 'object' && raw !== null) {
    if ('status' in raw) return raw
    if ('InProgress' in raw) return { status: 'in_progress', index: raw.InProgress.index }
    if ('Paused' in raw) return { status: 'paused', index: raw.Paused.index }
    if ('Expired' in raw) return { status: 'expired', index: raw.Expired.index }
  }
  return { status: 'not_started' }
}

// Wire representation from serde's externally-tagged Rust enum TickOutcome
export type RawTickOutcome =
  | 'None'
  | {
      QuestionTimedOut: { index: number; graded: GradedAnswer }
      revealed_question?: QuestionCard | null
    }
  | { SessionExpired: { index: number } }
  | TickOutcome

export type TickOutcome =
  | { type: 'none' }
  | {
      type: 'question_timed_out'
      index: number
      graded: GradedAnswer
      revealed_question?: QuestionCard | null
    }
  | { type: 'session_expired'; index: number }

export function parseTickOutcome(raw: RawTickOutcome): TickOutcome {
  if (raw === 'None') return { type: 'none' }
  if (typeof raw === 'object' && raw !== null) {
    if ('type' in raw) return raw as TickOutcome
    if ('QuestionTimedOut' in raw) {
      return {
        type: 'question_timed_out',
        index: raw.QuestionTimedOut.index,
        graded: raw.QuestionTimedOut.graded,
        revealed_question: raw.revealed_question ?? null,
      }
    }
    if ('SessionExpired' in raw) {
      return { type: 'session_expired', index: raw.SessionExpired.index }
    }
  }
  return { type: 'none' }
}

export interface SessionOptions {
  question_limit?: number
  new_card_limit?: number
  total_time_secs?: number
  seconds_per_question?: number
  affects_scheduling?: boolean
  pin_radius?: number
}

export interface CreateSessionRequest {
  mode: SessionMode
  exam_set_id?: string
  knowledge_card_ids?: string[]
  question_ids?: string[]
  options?: SessionOptions
}

export interface CreateSessionResponse {
  session_id: string
  state: SessionState
  question_count: number
}

export interface SessionSnapshot {
  session_id: string
  mode: SessionMode
  state: SessionState
  question: QuestionCard | null
  question_count: number
}

export interface SubmitAnswerResponse {
  state: SessionState
  grading: GradingOutcome | null
  revealed_question?: QuestionCard | null
}

export interface FinishSessionResponse {
  attempts_written: number
  score: Score
  graded?: GradedAnswer[]
  revealed_questions?: QuestionCard[]
}

export interface ScoreResponse {
  score: Score
  graded_count: number
}

export interface AttemptLog {
  id: string
  user_id: string
  card_id: string
  direction: AttemptDirection
  session_mode: SessionMode
  submitted_payload: Record<string, unknown>
  is_correct: boolean
  duration_ms: number
  confidence: Confidence
  error_tags: string[]
  created_at: string
  sync_cursor: number
  affects_scheduling: boolean
}

export interface CardScheduling {
  user_id: string
  card_id: string
  direction: AttemptDirection
  stability: number
  difficulty: number
  reps: number
  lapses: number
  state: SchedulingState
  last_review_at: string | null
  due_at: string | null
  desired_retention: number
  updated_at: string
}

export interface KnowledgeWeakness {
  card_id: string
  attempts: number
  error_rate: number
  hesitant_rate: number
  average_duration_ms: number
  score: number
}

export interface QuestionWeakness {
  question_id: string
  attempts: number
  error_rate: number
  hesitant_rate: number
  average_duration_ms: number
  score: number
  linked_card_ids: string[]
}

export interface ConfusablePair {
  question_id: string
  wrong_option: string
  correct_option: string
  count: number
}

export interface WeaknessReport {
  knowledge_cards: KnowledgeWeakness[]
  questions: QuestionWeakness[]
  confusable_pairs: ConfusablePair[]
}

export interface ApiMeta {
  version: string
  auth_mode: AuthMode
}

export interface AuthStatus {
  auth_mode: AuthMode
  needs_owner_setup: boolean
  authenticated: boolean
  role: UserRole | null
}

export interface AuthSession {
  user_id: string
  role: UserRole
}

// Proposals & Triage
export type ProposalStatus = 'pending' | 'approved' | 'rejected'

export type OperationKind =
  | 'resource'
  | 'anchor'
  | 'knowledge_card'
  | 'note'
  | 'exam_set'
  | 'question_group'
  | 'question_card'
  | 'edge'

export type ProposalTarget =
  | { resource: string }
  | { knowledge_card: string }
  | { note: string }
  | { exam_set: string }
  | { question_group: string }
  | { question_card: string }
  | { edge: string }

export function parseProposalTarget(target: ProposalTarget): {
  kind: Exclude<OperationKind, 'anchor'>
  id: string
} {
  if ('resource' in target) return { kind: 'resource', id: target.resource }
  if ('knowledge_card' in target) return { kind: 'knowledge_card', id: target.knowledge_card }
  if ('note' in target) return { kind: 'note', id: target.note }
  if ('exam_set' in target) return { kind: 'exam_set', id: target.exam_set }
  if ('question_group' in target) return { kind: 'question_group', id: target.question_group }
  if ('question_card' in target) return { kind: 'question_card', id: target.question_card }
  return { kind: 'edge', id: target.edge }
}

export interface QuestionGroup {
  id: string
  exam_set_id: string | null
  title: string | null
  shared_stem_blocks: ContentBlock[]
}

export interface Edge {
  id: string
  edge_type: EdgeType
  from: EdgeEndpoint
  to: EdgeEndpoint
  created_at?: string
}

// Discriminated union by `op` matching StoredOperation in crates/reinforce-core/src/proposal/batch.rs
export type BatchOperation =
  | { op: 'resource'; temp_ref?: string | null; id?: string; data: Resource }
  | { op: 'anchor'; temp_ref?: string | null; id?: string; data: Anchor }
  | { op: 'knowledge_card'; temp_ref?: string | null; id?: string; data: KnowledgeCard }
  | { op: 'note'; temp_ref?: string | null; id?: string; data: Note }
  | { op: 'exam_set'; temp_ref?: string | null; id?: string; data: ExamSet }
  | { op: 'question_group'; temp_ref?: string | null; id?: string; data: QuestionGroup }
  | { op: 'question_card'; temp_ref?: string | null; id?: string; data: QuestionCard }
  | { op: 'edge'; temp_ref?: string | null; id?: string; data: Edge }

export interface BatchPayload {
  format?: string
  actor?: unknown
  operations: BatchOperation[]
}

export interface Proposal {
  id: string
  target: ProposalTarget
  payload: BatchPayload
  submitted_by: string
  status: ProposalStatus
  review_note: string | null
  created_at: string
  updated_at: string
  sync_cursor: number
}

export type ValidationRule =
  | 'answer_provenance'
  | 'original_anti_fabrication'
  | 'type_grading_consistency'
  | 'count_conservation'
  | 'referential_integrity'
  | 'verification_lock'
  | 'temp_ref'
  | 'operation_shape'

export interface ValidationErrorItem {
  op_index: number | null
  temp_ref: string | null
  field_path: string
  rule: ValidationRule | string
  message: string
}

export interface ValidationReport {
  errors: ValidationErrorItem[]
}

export interface PinAdjustment {
  no?: number
  index?: number
  x: number
  y: number
  label_x: number
  label_y: number
}

export interface AdjustPinsRequest {
  resource_id?: string
  pins: PinAdjustment[]
}

export interface AdjustPinsResponse {
  id: string
  adjusted: number
}

export interface UpdateOperationRequest {
  data: BatchOperation['data']
}

// Export Center
export type ExportFormat = 'pdf' | 'markdown'
export type ExportPreset = 'standard' | 'goodnotes' | 'worksheet'

export type ExportScope =
  | { kind: 'note'; note_id: string }
  | { kind: 'subject'; subject: string }
  | { kind: 'weakness'; threshold: number }

export interface ExportRequest {
  format: ExportFormat
  preset?: ExportPreset
  scope: ExportScope
  web_base_url?: string
}

export interface ExportResult {
  blob: Blob
  filename: string
  contentType: string
}
