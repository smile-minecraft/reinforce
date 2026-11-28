import {
  AlertTriangle,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Clock,
  FileSearch,
  Flag,
  Info,
  ListChecks,
  Pause,
  Play,
  RotateCcw,
  Scale,
  Send,
  XCircle,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation, useParams } from 'react-router'
import { api, ApiError } from '../api/client'
import {
  useFinishSession,
  usePauseSession,
  useResumeSession,
  useSelectSessionQuestion,
  useStudySession,
  useSubmitAnswer,
  useTickSession,
} from '../api/hooks/useStudySession'
import type {
  AnswerSubmission,
  ContentBlock,
  FinishSessionResponse,
  GradingOutcome,
  NormalizedPoint,
  Pin,
  PinClick,
  QuestionCard,
  SessionState,
} from '../api/types'
import { BlockRenderer } from '../blocks/BlockRenderer'
import { AnchorDrawer } from '../components/AnchorDrawer'
import { OptionLinkedCards } from '../study/OptionLinkedCards'
import { PracticalCanvas } from '../study/PracticalCanvas'
import { loadSessionConfig, type StoredSessionConfig } from '../study/sessionConfig'
import { StudySummary, type SessionReviewEntry } from '../study/StudySummary'
import { RightSidebarSlot } from '../workspace/WorkspaceContext'
import { useWorkspace } from '../workspace/workspaceContextValue'
import {
  buildGroupedQuestionSidebar,
  buildQuestionUnits,
  CONFIDENCE_LEVELS_5,
  findUnitIndexBySessionIndex,
  mapConfidence5ToCore,
  type ConfidenceLevel5,
  type QuestionUnit,
} from '../workspace/workspaceLogic'

function formatTimer(secs: number): string {
  const clamped = Math.max(0, Math.ceil(secs))
  const m = Math.floor(clamped / 60)
  const s = clamped % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

function extractPracticalMedia(blocks: ContentBlock[]): {
  resourceId: string
  overlayResourceId: string | null
  pins: Pin[]
} | null {
  for (const b of blocks) {
    if (b.type === 'interactive_pins') {
      return {
        resourceId: b.resource_id,
        overlayResourceId: b.overlay_resource_id,
        pins: b.pins,
      }
    }
    if (b.type === 'image_occlusion') {
      return {
        resourceId: b.resource_id,
        overlayResourceId: null,
        pins: [],
      }
    }
  }
  return null
}

interface QuestionDraft {
  selectedKeys: string[]
  textAnswer: string
  pinClicks: PinClick[]
}

const EMPTY_DRAFT: QuestionDraft = {
  selectedKeys: [],
  textAnswer: '',
  pinClicks: [],
}

interface SubmittedFeedback {
  question: QuestionCard
  submission: AnswerSubmission
  grading: GradingOutcome | null
  timedOut?: boolean
}

interface LegacyReviewingState {
  sessionIndex: number
  question: QuestionCard
  nextState: SessionState
}

export function StudyRunnerPage() {
  const { id: sessionId = '' } = useParams()
  return <StudyRunnerContent key={sessionId} sessionId={sessionId} />
}

function StudyRunnerContent({ sessionId }: { sessionId: string }) {
  const location = useLocation()
  const { t } = useTranslation()
  const { openTab, setMobileRightOpen } = useWorkspace()

  const storedConfig = useMemo<StoredSessionConfig | null>(() => {
    const navState = location.state as Partial<StoredSessionConfig> | null
    const fromStorage = loadSessionConfig(sessionId)
    if (navState?.mode) {
      return {
        mode: navState.mode,
        secondsPerQuestion: navState.secondsPerQuestion ?? fromStorage?.secondsPerQuestion ?? null,
        totalTimeSecs: navState.totalTimeSecs ?? fromStorage?.totalTimeSecs ?? null,
      }
    }
    return fromStorage
  }, [location.state, sessionId])

  const {
    data: snapshot,
    isLoading,
    error: sessionError,
    refetch: refetchSession,
  } = useStudySession(sessionId)
  const pauseMutation = usePauseSession(sessionId)
  const resumeMutation = useResumeSession(sessionId)
  const tickMutation = useTickSession(sessionId)
  const selectMutation = useSelectSessionQuestion(sessionId)
  const submitMutation = useSubmitAnswer(sessionId)
  const finishMutation = useFinishSession(sessionId)

  // Local overrides for drafts & feedback (merged with server snapshot below)
  const [localDraftsByIndex, setLocalDraftsByIndex] = useState<Record<number, QuestionDraft>>({})
  const [localFeedbackByIndex, setLocalFeedbackByIndex] = useState<
    Record<number, SubmittedFeedback>
  >({})
  const [legacyReviewing, setLegacyReviewing] = useState<LegacyReviewingState | null>(null)
  const [confidence5, setConfidence5] = useState<ConfidenceLevel5>('certain')
  const [activeUnitIdx, setActiveUnitIdx] = useState(0)

  const [actionError, setActionError] = useState<string | null>(null)
  const [statusToast, setStatusToast] = useState<string | null>(null)
  const [activeAnchorId, setActiveAnchorId] = useState<string | null>(null)
  const [finishResult, setFinishResult] = useState<FinishSessionResponse | null>(null)

  const autoPausedByVisibilityRef = useRef(false)
  const [visibilityPausedNotice, setVisibilityPausedNotice] = useState(false)
  const scrollContainerRef = useRef<HTMLDivElement | null>(null)

  const mode = snapshot?.mode ?? storedConfig?.mode ?? 'daily_due'
  const isPractical = mode === 'practical_exam'
  const isFullExam = mode === 'full_exam'
  const perQuestionLimitSecs = isPractical ? (storedConfig?.secondsPerQuestion ?? 30) : null
  const totalExamLimitSecs = isFullExam ? (storedConfig?.totalTimeSecs ?? null) : null
  const isTimedMode = perQuestionLimitSecs !== null || totalExamLimitSecs !== null

  const [questionRemainingSecs, setQuestionRemainingSecs] = useState<number>(
    perQuestionLimitSecs ?? 30,
  )
  const [totalRemainingSecs, setTotalRemainingSecs] = useState<number>(totalExamLimitSecs ?? 0)
  const tickInFlightRef = useRef(false)
  const lastSyncRef = useRef<number>(0)

  const sessionState = snapshot?.state
  const isPaused = sessionState?.status === 'paused'
  const isExpired = sessionState?.status === 'expired'
  const isSubmitted = sessionState?.status === 'submitted'

  const isFullListMode = Boolean(snapshot?.questions && snapshot.questions.length > 0)
  const serverIndex =
    sessionState && (sessionState.status === 'in_progress' || sessionState.status === 'paused')
      ? sessionState.index
      : 0

  // Build QuestionUnits from snapshot.questions (or fallback to single snapshot.question for older servers)
  const allQuestions = useMemo<QuestionCard[]>(() => {
    if (isFullListMode && snapshot?.questions) {
      return snapshot.questions
    }
    if (legacyReviewing) {
      return [legacyReviewing.question]
    }
    return snapshot?.question ? [snapshot.question] : []
  }, [isFullListMode, snapshot, legacyReviewing])

  const units = useMemo<QuestionUnit[]>(() => {
    if (isFullListMode) {
      return buildQuestionUnits(
        allQuestions,
        snapshot?.question_groups ?? {},
        snapshot?.exam_sets ?? {},
      )
    }
    if (legacyReviewing) {
      return buildQuestionUnits(
        [legacyReviewing.question],
        snapshot?.question_groups ?? {},
        snapshot?.exam_sets ?? {},
        legacyReviewing.sessionIndex,
      )
    }
    return buildQuestionUnits(
      allQuestions,
      snapshot?.question_groups ?? {},
      snapshot?.exam_sets ?? {},
      serverIndex,
    )
  }, [
    isFullListMode,
    allQuestions,
    legacyReviewing,
    snapshot?.question_groups,
    snapshot?.exam_sets,
    serverIndex,
  ])

  const sidebarBuckets = useMemo(
    () => buildGroupedQuestionSidebar(units, t('workspace.sidebar.ungroupedExam')),
    [units, t],
  )

  // Derive merged server + local feedback & drafts (no setState inside useEffect)
  const feedbackByIndex = useMemo<Record<number, SubmittedFeedback>>(() => {
    const merged: Record<number, SubmittedFeedback> = {}
    if (isFullListMode) {
      const answers = snapshot?.answers ?? []
      const graded = snapshot?.graded ?? []
      for (const ans of answers) {
        const matchedGraded = graded.find((g) => g.record.index === ans.index)
        const q = allQuestions[ans.index]
        if (q) {
          merged[ans.index] = {
            question: q,
            submission: ans.submission,
            grading: matchedGraded?.outcome ?? null,
            timedOut: ans.timed_out,
          }
        }
      }
    }
    return { ...merged, ...localFeedbackByIndex }
  }, [isFullListMode, snapshot, allQuestions, localFeedbackByIndex])

  const draftsByIndex = useMemo<Record<number, QuestionDraft>>(() => {
    const merged: Record<number, QuestionDraft> = {}
    if (isFullListMode) {
      for (const ans of snapshot?.answers ?? []) {
        if (ans.submission.kind === 'keys') {
          merged[ans.index] = { ...EMPTY_DRAFT, selectedKeys: ans.submission.values }
        } else if (ans.submission.kind === 'text') {
          merged[ans.index] = { ...EMPTY_DRAFT, textAnswer: ans.submission.value }
        } else if (ans.submission.kind === 'pin_clicks') {
          merged[ans.index] = { ...EMPTY_DRAFT, pinClicks: ans.submission.clicks }
        }
      }
    }
    return { ...merged, ...localDraftsByIndex }
  }, [isFullListMode, snapshot, localDraftsByIndex])

  // Keep activeUnitIdx clamped
  const clampedUnitIdx = isFullListMode
    ? Math.min(activeUnitIdx, Math.max(0, units.length - 1))
    : 0
  const currentUnit: QuestionUnit | null = units[clampedUnitIdx] ?? null

  const currentUnitFullyGraded = Boolean(
    currentUnit &&
      currentUnit.items.length > 0 &&
      currentUnit.items.every((item) => Boolean(feedbackByIndex[item.sessionIndex]?.grading)),
  )

  const answeredCount = Object.keys(feedbackByIndex).length
  const totalQuestionCount = snapshot?.question_count ?? allQuestions.length

  const navigateToUnit = useCallback(
    async (targetUnitIndex: number) => {
      if (!isFullListMode) return
      if (targetUnitIndex < 0 || targetUnitIndex >= units.length) return
      const targetUnit = units[targetUnitIndex]
      setActiveUnitIdx(targetUnitIndex)
      setActionError(null)
      setMobileRightOpen(false)
      scrollContainerRef.current?.scrollTo({ top: 0, behavior: 'smooth' })
      if (perQuestionLimitSecs !== null) {
        setQuestionRemainingSecs(perQuestionLimitSecs)
      }
      const firstSessionIdx = targetUnit.items[0]?.sessionIndex
      if (
        typeof firstSessionIdx === 'number' &&
        (sessionState?.status === 'in_progress' || sessionState?.status === 'paused')
      ) {
        try {
          await selectMutation.mutateAsync(firstSessionIdx)
        } catch {
          // Graceful fallback if older server doesn't support /select
        }
      }
    },
    [
      isFullListMode,
      units,
      perQuestionLimitSecs,
      sessionState?.status,
      selectMutation,
      setMobileRightOpen,
    ],
  )

  const handleNextUnit = useCallback(async () => {
    if (isFullListMode) {
      await navigateToUnit(clampedUnitIdx + 1)
      return
    }
    if (legacyReviewing) {
      setLegacyReviewing(null)
      setActionError(null)
      scrollContainerRef.current?.scrollTo({ top: 0, behavior: 'smooth' })
      if (perQuestionLimitSecs !== null) {
        setQuestionRemainingSecs(perQuestionLimitSecs)
      }
      await refetchSession()
    }
  }, [
    isFullListMode,
    navigateToUnit,
    clampedUnitIdx,
    legacyReviewing,
    perQuestionLimitSecs,
    refetchSession,
  ])

  const handleFinish = useCallback(async () => {
    setActionError(null)
    try {
      const result = await finishMutation.mutateAsync()
      setFinishResult(result)
    } catch (err) {
      setActionError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }, [finishMutation, t])

  const runServerTick = useCallback(async () => {
    if (!sessionId || tickInFlightRef.current || finishResult) return
    tickInFlightRef.current = true
    try {
      const outcome = await tickMutation.mutateAsync()
      lastSyncRef.current = Date.now()

      if (outcome.type === 'question_timed_out') {
        const revealed =
          outcome.revealed_question ??
          (isFullListMode ? allQuestions[outcome.index] : snapshot?.question) ??
          null
        const alreadyGraded = Boolean(feedbackByIndex[outcome.index]?.grading)
        if (revealed && !alreadyGraded) {
          setLocalFeedbackByIndex((prev) => {
            if (prev[outcome.index]?.grading) return prev
            return {
              ...prev,
              [outcome.index]: {
                question: revealed,
                submission: { kind: 'timed_out' },
                grading: outcome.graded.outcome,
                timedOut: true,
              },
            }
          })
        }
        const refreshed = await refetchSession()
        const st = refreshed.data?.state
        if (isFullListMode) {
          if (!alreadyGraded && st && (st.status === 'in_progress' || st.status === 'paused')) {
            setActiveUnitIdx(findUnitIndexBySessionIndex(units, st.index))
            if (perQuestionLimitSecs !== null) {
              setQuestionRemainingSecs(perQuestionLimitSecs)
            }
          }
        } else if (revealed && !alreadyGraded) {
          setLegacyReviewing({
            sessionIndex: outcome.index,
            question: revealed,
            nextState: st ?? { status: 'submitted' },
          })
        }
      } else if (outcome.type === 'session_expired') {
        await refetchSession()
      }
    } catch {
      // Transient tick errors retry next cycle
    } finally {
      tickInFlightRef.current = false
    }
  }, [
    sessionId,
    finishResult,
    tickMutation,
    isFullListMode,
    allQuestions,
    snapshot,
    feedbackByIndex,
    refetchSession,
    units,
    perQuestionLimitSecs,
  ])

  const shouldRunTimer =
    isTimedMode &&
    sessionState?.status === 'in_progress' &&
    !finishResult &&
    !(isPractical && (currentUnitFullyGraded || legacyReviewing !== null))

  // Countdown timer loop (stops automatically in practical_exam once current unit is graded)
  useEffect(() => {
    if (!shouldRunTimer) {
      return
    }
    let lastFrame = performance.now()
    lastSyncRef.current = Date.now()
    const timer = window.setInterval(() => {
      const now = performance.now()
      const dt = (now - lastFrame) / 1000
      lastFrame = now

      if (perQuestionLimitSecs !== null) {
        setQuestionRemainingSecs((prev) => {
          const next = Math.max(0, prev - dt)
          if (next <= 0 && prev > 0) void runServerTick()
          return next
        })
      }
      if (totalExamLimitSecs !== null) {
        setTotalRemainingSecs((prev) => {
          const next = Math.max(0, prev - dt)
          if (next <= 0 && prev > 0) void runServerTick()
          return next
        })
      }
      if (Date.now() - lastSyncRef.current >= 5000) {
        void runServerTick()
      }
    }, 250)
    return () => window.clearInterval(timer)
  }, [shouldRunTimer, perQuestionLimitSecs, totalExamLimitSecs, runServerTick])

  // Visibility auto-pause
  useEffect(() => {
    if (!isTimedMode || !sessionId || finishResult) return
    function onVisibilityChange() {
      if (document.hidden) {
        if (sessionState?.status === 'in_progress') {
          autoPausedByVisibilityRef.current = true
          setVisibilityPausedNotice(true)
          void api.pauseSession(sessionId).then(() => refetchSession()).catch(() => {})
        }
      } else if (autoPausedByVisibilityRef.current) {
        autoPausedByVisibilityRef.current = false
        void api.resumeSession(sessionId).then(() => refetchSession()).catch(() => {})
      }
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => document.removeEventListener('visibilitychange', onVisibilityChange)
  }, [isTimedMode, sessionId, sessionState?.status, finishResult, refetchSession])

  function updateDraft(sessionIndex: number, updater: (prev: QuestionDraft) => QuestionDraft) {
    setLocalDraftsByIndex((prev) => ({
      ...prev,
      [sessionIndex]: updater(prev[sessionIndex] ?? draftsByIndex[sessionIndex] ?? EMPTY_DRAFT),
    }))
  }

  function buildSubmissionFor(question: QuestionCard, draft: QuestionDraft): AnswerSubmission | null {
    if (question.grading_mode === 'voided') {
      return question.options.length > 0
        ? {
            kind: 'keys',
            values: draft.selectedKeys.length > 0 ? draft.selectedKeys : [question.options[0].key],
          }
        : { kind: 'text', value: draft.textAnswer.trim() || '-' }
    }
    switch (question.question_type) {
      case 'single_choice':
      case 'multiple_choice':
        return draft.selectedKeys.length > 0 ? { kind: 'keys', values: draft.selectedKeys } : null
      case 'pin_locate':
        return draft.pinClicks.length > 0 ? { kind: 'pin_clicks', clicks: draft.pinClicks } : null
      case 'short_answer':
      case 'vocab':
      case 'image_qa':
      case 'image_occlusion':
      case 'pin_identify':
        return draft.textAnswer.trim().length > 0
          ? { kind: 'text', value: draft.textAnswer.trim() }
          : null
    }
  }

  // Can submit if at least one question in currentUnit has a valid draft
  const submittableItems = useMemo(() => {
    if (!currentUnit || sessionState?.status !== 'in_progress' || legacyReviewing !== null) {
      return []
    }
    return currentUnit.items.filter(({ sessionIndex, question }) => {
      const sub = buildSubmissionFor(question, draftsByIndex[sessionIndex] ?? EMPTY_DRAFT)
      if (!sub) return false
      if (!isFullExam && feedbackByIndex[sessionIndex]?.grading) return false
      return true
    })
  }, [currentUnit, draftsByIndex, feedbackByIndex, isFullExam, sessionState?.status, legacyReviewing])

  async function handleSubmitCurrentUnit() {
    if (!currentUnit || submittableItems.length === 0) return
    setActionError(null)
    const { confidence, impliedErrorTag } = mapConfidence5ToCore(confidence5)
    const errorTags = impliedErrorTag ? [impliedErrorTag] : undefined

    try {
      let lastRes: Awaited<ReturnType<typeof submitMutation.mutateAsync>> | null = null
      let lastSubmitted: { sessionIndex: number; question: QuestionCard } | null = null

      for (const { sessionIndex, question } of submittableItems) {
        const submission = buildSubmissionFor(question, draftsByIndex[sessionIndex] ?? EMPTY_DRAFT)
        if (!submission) continue

        const res = await submitMutation.mutateAsync({
          index: isFullListMode ? sessionIndex : undefined,
          submission,
          confidence,
          error_tags: errorTags,
        })
        lastRes = res
        lastSubmitted = { sessionIndex, question }

        setLocalFeedbackByIndex((prev) => ({
          ...prev,
          [sessionIndex]: {
            question: res.revealed_question ?? question,
            submission,
            grading: res.grading,
          },
        }))
      }

      if (isFullListMode) {
        if (isFullExam) {
          setStatusToast(t('study.runner.recorded'))
          window.setTimeout(() => setStatusToast(null), 1600)
        }
        await refetchSession()
      } else if (lastRes && lastSubmitted) {
        if (lastRes.grading !== null) {
          // Legacy single-question server already advanced state.index; keep feedback visible until user clicks Next
          setLegacyReviewing({
            sessionIndex: lastSubmitted.sessionIndex,
            question: lastRes.revealed_question ?? lastSubmitted.question,
            nextState: lastRes.state,
          })
        } else {
          // Legacy full_exam deferred grading: advance to next question cleanly
          setStatusToast(t('study.runner.recorded'))
          window.setTimeout(() => setStatusToast(null), 1600)
          await refetchSession()
        }
      }
    } catch (err) {
      setActionError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  // Summary entries built from finishResult + local feedback
  const summaryEntries = useMemo<SessionReviewEntry[]>(() => {
    if (!finishResult) return []
    const revealed = finishResult.revealed_questions ?? []
    const gradedList = finishResult.graded ?? []
    if (gradedList.length > 0) {
      const entries: SessionReviewEntry[] = []
      for (const g of gradedList) {
        const question =
          revealed[g.record.index] ??
          feedbackByIndex[g.record.index]?.question ??
          (isFullListMode ? allQuestions[g.record.index] : undefined)
        if (!question) continue
        entries.push({
          index: g.record.index,
          question,
          submission: g.record.submission,
          confidence: g.record.confidence,
          errorTags: [],
          grading: g.outcome,
        })
      }
      return entries
    }
    return Object.entries(feedbackByIndex)
      .map(([idxStr, fb]) => ({
        index: Number(idxStr),
        question: fb.question,
        submission: fb.submission,
        confidence: mapConfidence5ToCore(confidence5).confidence,
        errorTags: [],
        grading: fb.grading,
      }))
      .sort((a, b) => a.index - b.index)
  }, [finishResult, isFullListMode, allQuestions, feedbackByIndex, confidence5])

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
        {t('common.loading')}
      </div>
    )
  }

  if (sessionError || !snapshot) {
    return (
      <div className="p-6">
        <div className="rounded-xl bg-bad-soft p-5 text-sm text-bad">
          {sessionError instanceof Error ? sessionError.message : t('common.errorGeneric')}
        </div>
        <button
          type="button"
          onClick={() =>
            openTab({ id: 'study', kind: 'study', path: '/study', title: t('workspace.tabs.study') })
          }
          className="mt-4 inline-flex h-10 items-center rounded-xl border border-border bg-surface px-4 text-sm font-medium text-ink"
        >
          {t('study.runner.summary.backToStudy')}
        </button>
      </div>
    )
  }

  if (finishResult) {
    return (
      <div className="rf-pane-scroll flex-1 overflow-y-auto">
        <StudySummary
          score={finishResult.score}
          attemptsWritten={finishResult.attempts_written}
          entries={summaryEntries}
          onOpenAnchor={setActiveAnchorId}
        />
        <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
      </div>
    )
  }

  // Breadcrumb & Tags matching Sketch 1: "XX科目 / XX年 / 第XX題" + "#標籤"
  const breadcrumbParts: string[] = []
  if (currentUnit) {
    breadcrumbParts.push(currentUnit.subject)
    if (currentUnit.examYear) breadcrumbParts.push(`${currentUnit.examYear}`)
    else if (currentUnit.examTitle) breadcrumbParts.push(currentUnit.examTitle)

    const nums = currentUnit.items.map((item) => item.question.question_no ?? item.sessionIndex + 1)
    if (nums.length > 1) {
      breadcrumbParts.push(
        t('workspace.sidebar.questionRange', { start: nums[0], end: nums[nums.length - 1] }),
      )
    } else if (nums.length === 1) {
      breadcrumbParts.push(t('workspace.sidebar.questionNumber', { no: nums[0] }))
    }
  }

  const unitTags = currentUnit
    ? Array.from(new Set(currentUnit.items.flatMap((i) => i.question.topics)))
    : []

  const timerWarning =
    (perQuestionLimitSecs !== null && !currentUnitFullyGraded && questionRemainingSecs <= 5) ||
    (totalExamLimitSecs !== null && totalRemainingSecs <= 60)

  const hasPrevUnit = isFullListMode && clampedUnitIdx > 0
  const hasNextUnit = isFullListMode
    ? clampedUnitIdx < units.length - 1
    : Boolean(
        legacyReviewing &&
          legacyReviewing.nextState.status !== 'submitted' &&
          legacyReviewing.nextState.status !== 'expired',
      )

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface">
      {/* Right Sidebar: Grouped Question List (Sketch 1: 分組題目列表) */}
      <RightSidebarSlot>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 border-b border-border px-3.5 py-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5 text-xs font-semibold text-ink">
                <ListChecks className="h-3.5 w-3.5 text-accent" />
                <span>{t('workspace.sidebar.questionList')}</span>
              </div>
              <span className="rounded-full bg-subtle px-2 py-0.5 font-mono text-[10px] font-medium text-ink-muted">
                {t('workspace.sidebar.answeredCount', {
                  answered: answeredCount,
                  total: totalQuestionCount,
                })}
              </span>
            </div>
          </div>

          <div className="rf-pane-scroll flex-1 space-y-3 overflow-y-auto p-3">
            {sidebarBuckets.map((subjectBucket) => (
              <div key={subjectBucket.subject} className="space-y-1.5">
                <div className="px-1 text-xs font-semibold text-ink">{subjectBucket.subject}</div>

                {subjectBucket.exams.map((examBucket) => (
                  <div key={examBucket.key} className="space-y-1 pl-2">
                    <div className="flex items-center gap-1.5 px-1 text-[11px] font-medium text-ink-muted">
                      <span className="h-1.5 w-1.5 rounded-full border border-ink-muted/60" />
                      <span className="truncate">{examBucket.label}</span>
                    </div>

                    <ul className="space-y-0.5 pl-3">
                      {examBucket.units.map((unit) => {
                        const isActive = unit.unitIndex === clampedUnitIdx
                        const nums = unit.items.map(
                          (item) => item.question.question_no ?? item.sessionIndex + 1,
                        )
                        const label =
                          nums.length > 1
                            ? t('workspace.sidebar.questionRange', {
                                start: nums[0],
                                end: nums[nums.length - 1],
                              })
                            : t('workspace.sidebar.questionNumber', { no: nums[0] ?? 1 })

                        const unitFeedbacks = unit.items
                          .map((item) => feedbackByIndex[item.sessionIndex])
                          .filter(Boolean)
                        const allAnswered = unitFeedbacks.length === unit.items.length
                        const anyWrong = unitFeedbacks.some(
                          (fb) => fb.grading && !fb.grading.voided && !fb.grading.is_correct,
                        )
                        const allCorrect =
                          allAnswered &&
                          unitFeedbacks.every((fb) => fb.grading && (fb.grading.is_correct || fb.grading.voided))

                        let dotClass = 'border border-ink-muted/60 bg-transparent'
                        if (allCorrect) dotClass = 'bg-ok'
                        else if (anyWrong) dotClass = 'bg-bad'
                        else if (unitFeedbacks.length > 0) dotClass = 'bg-accent'

                        return (
                          <li key={unit.unitId}>
                            <button
                              type="button"
                              onClick={() => void navigateToUnit(unit.unitIndex)}
                              className={`flex w-full items-center justify-between gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs transition-colors ${
                                isActive
                                  ? 'bg-accent-soft font-semibold text-accent'
                                  : 'text-ink-muted hover:bg-subtle hover:text-ink'
                              }`}
                            >
                              <span className="flex min-w-0 items-center gap-2">
                                <span className={`h-2 w-2 shrink-0 rounded-full ${dotClass}`} />
                                <span className="truncate">{label}</span>
                              </span>

                              {unit.items.length > 1 && (
                                <span className="shrink-0 rounded-full bg-surface px-1.5 py-0.5 text-[10px] font-medium text-ink-muted">
                                  {t('workspace.sidebar.questionGroupBadge', {
                                    count: unit.items.length,
                                  })}
                                </span>
                              )}
                            </button>
                          </li>
                        )
                      })}
                    </ul>
                  </div>
                ))}
              </div>
            ))}
          </div>
        </div>
      </RightSidebarSlot>

      {/* Center Frame Top Meta Header: Breadcrumb + #Tags + Controls (Sketch 1) */}
      <header className="shrink-0 border-b border-border bg-surface px-5 py-3 sm:px-8">
        <div className="mx-auto flex max-w-3xl flex-wrap items-center justify-between gap-2.5">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="text-sm font-semibold tracking-tight text-ink">
              {breadcrumbParts.join(' / ')}
            </span>

            {unitTags.map((tag) => (
              <span
                key={tag}
                className="inline-flex items-center rounded-full border border-border bg-canvas px-2.5 py-0.5 text-xs font-medium text-ink-muted"
              >
                # {tag}
              </span>
            ))}
          </div>

          <div className="flex items-center gap-2">
            {perQuestionLimitSecs !== null && (
              <div
                className={`inline-flex h-8 items-center gap-1.5 rounded-full border px-3 font-mono text-xs font-bold tabular-nums ${
                  timerWarning
                    ? 'border-bad bg-bad-soft text-bad'
                    : 'border-border bg-canvas text-ink'
                }`}
              >
                <Clock className="h-3.5 w-3.5" />
                <span>{formatTimer(questionRemainingSecs)}</span>
              </div>
            )}

            {totalExamLimitSecs !== null && (
              <div
                className={`inline-flex h-8 items-center gap-1.5 rounded-full border px-3 font-mono text-xs font-bold tabular-nums ${
                  timerWarning
                    ? 'border-bad bg-bad-soft text-bad'
                    : 'border-border bg-canvas text-ink'
                }`}
              >
                <Clock className="h-3.5 w-3.5" />
                <span>{formatTimer(totalRemainingSecs)}</span>
              </div>
            )}

            {(sessionState?.status === 'in_progress' || isPaused) && (
              <button
                type="button"
                onClick={() => (isPaused ? resumeMutation.mutate() : pauseMutation.mutate())}
                disabled={pauseMutation.isPending || resumeMutation.isPending}
                className="inline-flex h-8 items-center gap-1 rounded-full border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
              >
                {isPaused ? <Play className="h-3 w-3" /> : <Pause className="h-3 w-3" />}
                <span>{isPaused ? t('study.runner.resume') : t('study.runner.pause')}</span>
              </button>
            )}

            <button
              type="button"
              onClick={() => void handleFinish()}
              disabled={finishMutation.isPending}
              className="inline-flex h-8 items-center gap-1.5 rounded-full border border-accent/40 bg-accent-soft px-3 text-xs font-semibold text-accent hover:bg-accent hover:text-white disabled:opacity-50"
            >
              <Flag className="h-3.5 w-3.5" />
              <span>{t('study.runner.finish')}</span>
            </button>
          </div>
        </div>
      </header>

      {/* Scrollable Middle Question Stage (Whole QuestionGroup in One View!) */}
      <div ref={scrollContainerRef} className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-6 sm:px-8">
        <div className="mx-auto max-w-3xl space-y-6">
          {isFullExam && (
            <div className="rounded-xl border border-accent/30 bg-accent-soft/70 px-3.5 py-2 text-xs text-accent">
              {t('study.runner.deferredBanner')}
            </div>
          )}

          {visibilityPausedNotice && (
            <div className="flex items-center justify-between gap-2 rounded-xl border border-warn/40 bg-warn-soft px-3.5 py-2 text-xs text-warn">
              <span>{t('study.runner.pausedVisibility')}</span>
              <button
                type="button"
                onClick={() => setVisibilityPausedNotice(false)}
                className="font-semibold underline"
              >
                {t('common.close')}
              </button>
            </div>
          )}

          {statusToast && (
            <div className="rounded-xl bg-ok-soft px-3.5 py-2 text-xs font-medium text-ok">
              {statusToast}
            </div>
          )}

          {actionError && (
            <div className="rounded-xl bg-bad-soft px-4 py-3 text-sm text-bad">{actionError}</div>
          )}

          {isPaused ? (
            <section className="rounded-2xl border border-border bg-canvas/50 p-8 text-center">
              <Pause className="mx-auto h-10 w-10 text-accent" />
              <h2 className="mt-3 text-lg font-semibold text-ink">{t('study.runner.pausedTitle')}</h2>
              <p className="mt-1 text-sm text-ink-muted">{t('study.runner.pausedManual')}</p>
              <button
                type="button"
                onClick={() => resumeMutation.mutate()}
                className="mt-5 inline-flex h-11 items-center gap-2 rounded-full bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
              >
                <Play className="h-4 w-4 fill-current" />
                <span>{t('study.runner.resume')}</span>
              </button>
            </section>
          ) : isExpired ? (
            <section className="rounded-2xl border border-warn/40 bg-canvas/50 p-8 text-center">
              <AlertTriangle className="mx-auto h-10 w-10 text-warn" />
              <h2 className="mt-3 text-lg font-semibold text-ink">{t('study.runner.expiredTitle')}</h2>
              <p className="mt-1 text-sm text-ink-muted">{t('study.runner.expiredDesc')}</p>
              <button
                type="button"
                onClick={() => void handleFinish()}
                disabled={finishMutation.isPending}
                className="mt-5 inline-flex h-11 items-center gap-2 rounded-full bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
              >
                <Flag className="h-4 w-4" />
                <span>{t('study.runner.finish')}</span>
              </button>
            </section>
          ) : !legacyReviewing && isSubmitted ? (
            <section className="rounded-2xl border border-accent/30 bg-canvas/50 p-8 text-center">
              <CheckCircle2 className="mx-auto h-10 w-10 text-ok" />
              <h2 className="mt-3 text-lg font-semibold text-ink">{t('study.runner.recorded')}</h2>
              <button
                type="button"
                onClick={() => void handleFinish()}
                disabled={finishMutation.isPending}
                className="mt-5 inline-flex h-11 items-center gap-2 rounded-full bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
              >
                <Flag className="h-4 w-4" />
                <span>{t('study.runner.finish')}</span>
              </button>
            </section>
          ) : currentUnit ? (
            <div className="space-y-6">
              {/* Shared QuestionGroup Vignette (shown above ALL child questions of this group) */}
              {currentUnit.group && (
                <section className="rounded-2xl border border-accent/30 bg-canvas/80 p-5">
                  <div className="mb-2 flex items-center gap-2 text-xs font-semibold text-accent">
                    <span>
                      {currentUnit.group.title ??
                        t('workspace.sidebar.questionGroupBadge', {
                          count: currentUnit.items.length,
                        })}
                    </span>
                  </div>
                  <div className="space-y-3">
                    {currentUnit.group.shared_stem_blocks.map((block, bIdx) => (
                      <BlockRenderer
                        key={`${currentUnit.group!.id}-shared-${bIdx}`}
                        block={block}
                      />
                    ))}
                  </div>
                </section>
              )}

              {/* All Questions belonging to this Unit (1 for standalone, N for QuestionGroup) */}
              {currentUnit.items.map(({ sessionIndex, question }) => {
                const draft = draftsByIndex[sessionIndex] ?? EMPTY_DRAFT
                const fb = feedbackByIndex[sessionIndex]
                const activeQ = fb?.question ?? question
                const locked = Boolean(fb?.grading) && !isFullExam

                const media = extractPracticalMedia(activeQ.stem_blocks)
                const showCanvas = Boolean(
                  media &&
                    (activeQ.question_type === 'pin_locate' ||
                      activeQ.question_type === 'pin_identify' ||
                      activeQ.question_type === 'image_qa'),
                )
                const nonCanvasBlocks = activeQ.stem_blocks.filter((b) =>
                  showCanvas ? b.type !== 'interactive_pins' && b.type !== 'image_occlusion' : true,
                )

                const requiredPinNos =
                  activeQ.question_type === 'pin_locate'
                    ? (media?.pins.map((p) => p.no).length ? media!.pins.map((p) => p.no) : [1])
                    : []
                const nextUnplacedPinNo =
                  requiredPinNos.find((no) => !draft.pinClicks.some((c) => c.no === no)) ??
                  requiredPinNos[requiredPinNos.length - 1] ??
                  null

                const acceptedKeys = new Set<string>([
                  ...(activeQ.answer ?? []),
                  ...(activeQ.grading_mode === 'any_accepted'
                    ? (activeQ.accepted_answers ?? []).flat()
                    : []),
                ])

                return (
                  <article
                    key={`${activeQ.id}-${sessionIndex}`}
                    className={`space-y-4 ${
                      currentUnit.items.length > 1
                        ? 'rounded-2xl border border-border bg-canvas/30 p-5'
                        : ''
                    }`}
                  >
                    {/* Question Header badges */}
                    <div className="flex flex-wrap items-center gap-2 text-xs">
                      {currentUnit.items.length > 1 && (
                        <span className="rounded-full bg-accent-soft px-2.5 py-0.5 font-semibold text-accent">
                          {t('workspace.sidebar.questionNumber', {
                            no: activeQ.question_no ?? sessionIndex + 1,
                          })}
                        </span>
                      )}
                      <span className="rounded-full bg-subtle px-2.5 py-0.5 font-medium text-ink-muted">
                        {t(`study.runner.questionTypes.${activeQ.question_type}`)}
                      </span>
                      {activeQ.grading_mode === 'any_accepted' && (
                        <span className="inline-flex items-center gap-1 rounded-full bg-warn-soft px-2.5 py-0.5 font-medium text-warn">
                          <Scale className="h-3 w-3" />
                          {t('study.runner.grading.anyAccepted')}
                        </span>
                      )}
                      {activeQ.grading_mode === 'voided' && (
                        <span className="inline-flex items-center gap-1 rounded-full bg-subtle px-2.5 py-0.5 font-medium text-ink-muted">
                          <Info className="h-3 w-3" />
                          {t('study.runner.grading.voided')}
                        </span>
                      )}
                    </div>

                    {/* Practical / Image Canvas */}
                    {showCanvas && media && (
                      <PracticalCanvas
                        resourceId={media.resourceId}
                        overlayResourceId={media.overlayResourceId}
                        pins={media.pins}
                        mode={
                          activeQ.question_type === 'pin_locate'
                            ? 'locate'
                            : activeQ.question_type === 'pin_identify'
                              ? 'identify'
                              : 'view'
                        }
                        placedClicks={draft.pinClicks}
                        activePinNo={nextUnplacedPinNo}
                        revealPins={Boolean(fb?.grading)}
                        onPlacePin={(point: NormalizedPoint) => {
                          if (locked || nextUnplacedPinNo === null) return
                          updateDraft(sessionIndex, (prev) => {
                            const targetNo =
                              requiredPinNos.find(
                                (no) => !prev.pinClicks.some((c) => c.no === no),
                              ) ?? requiredPinNos[requiredPinNos.length - 1]
                            const filtered = prev.pinClicks.filter((c) => c.no !== targetNo)
                            return { ...prev, pinClicks: [...filtered, { no: targetNo, point }] }
                          })
                        }}
                      />
                    )}

                    {/* Stem Blocks */}
                    <div className="space-y-3 text-base leading-relaxed text-ink">
                      {nonCanvasBlocks.map((block, bIdx) => (
                        <BlockRenderer
                          key={`${activeQ.id}-stem-${bIdx}`}
                          block={block}
                          hidePinTerms={!fb?.grading}
                        />
                      ))}
                    </div>

                    {/* Pill-shaped Choice Options (Sketch 1: rounded full/2xl wide pills A/B/C/D) */}
                    {activeQ.options.length > 0 &&
                      (activeQ.question_type === 'single_choice' ||
                        activeQ.question_type === 'multiple_choice') && (
                        <div className="space-y-3 pt-1">
                          {activeQ.options.map((opt) => {
                            const isSelected = draft.selectedKeys.includes(opt.key)
                            const hasGrading = Boolean(fb?.grading)
                            const isCorrectOpt = hasGrading && acceptedKeys.has(opt.key)
                            const isWrongSelected =
                              hasGrading &&
                              isSelected &&
                              !fb?.grading?.voided &&
                              acceptedKeys.size > 0 &&
                              !acceptedKeys.has(opt.key)

                            let pillClass =
                              'border-border bg-surface hover:border-accent/60 hover:bg-canvas/70'
                            if (hasGrading) {
                              if (isCorrectOpt) pillClass = 'border-ok bg-ok-soft/70'
                              else if (isWrongSelected) pillClass = 'border-bad bg-bad-soft/70'
                              else pillClass = 'border-border bg-surface opacity-80'
                            } else if (isSelected) {
                              pillClass = 'border-accent bg-accent-soft/80 shadow-2xs'
                            }

                            return (
                              <div
                                key={opt.key}
                                className={`rounded-3xl border px-5 py-3.5 transition-all ${pillClass}`}
                              >
                                <button
                                  type="button"
                                  disabled={locked}
                                  onClick={() => {
                                    updateDraft(sessionIndex, (prev) => {
                                      if (activeQ.question_type === 'single_choice') {
                                        return { ...prev, selectedKeys: [opt.key] }
                                      }
                                      const exists = prev.selectedKeys.includes(opt.key)
                                      return {
                                        ...prev,
                                        selectedKeys: exists
                                          ? prev.selectedKeys.filter((k) => k !== opt.key)
                                          : [...prev.selectedKeys, opt.key].sort(),
                                      }
                                    })
                                  }}
                                  className="flex w-full items-start gap-3 text-left disabled:cursor-default"
                                >
                                  <span
                                    className={`mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full border font-mono text-xs font-bold ${
                                      isCorrectOpt
                                        ? 'border-ok bg-ok text-white'
                                        : isWrongSelected
                                          ? 'border-bad bg-bad text-white'
                                          : isSelected
                                            ? 'border-accent bg-accent text-white'
                                            : 'border-border bg-canvas text-ink-muted'
                                    }`}
                                  >
                                    {opt.key}
                                  </span>
                                  <span className="flex-1 text-sm font-medium leading-relaxed text-ink sm:text-[15px]">
                                    {opt.content}
                                  </span>
                                </button>

                                {hasGrading && opt.explanation && (
                                  <p className="mt-2 pl-10 text-xs leading-relaxed text-ink-muted">
                                    {opt.explanation}
                                  </p>
                                )}

                                {hasGrading && opt.linked_card_ids.length > 0 && (
                                  <div className="mt-2 pl-10">
                                    <OptionLinkedCards
                                      cardIds={opt.linked_card_ids}
                                      onOpenAnchor={setActiveAnchorId}
                                    />
                                  </div>
                                )}
                              </div>
                            )
                          })}
                        </div>
                      )}

                    {/* Pin Locate Status Bar */}
                    {activeQ.question_type === 'pin_locate' && !locked && (
                      <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-border bg-canvas px-4 py-3 text-xs text-ink">
                        <span>
                          {draft.pinClicks.length >= requiredPinNos.length
                            ? t('study.runner.pinLocateAllPlaced')
                            : t('study.runner.pinLocatePrompt', { no: nextUnplacedPinNo ?? 1 })}
                        </span>
                        {draft.pinClicks.length > 0 && (
                          <button
                            type="button"
                            onClick={() =>
                              updateDraft(sessionIndex, (prev) => ({ ...prev, pinClicks: [] }))
                            }
                            className="inline-flex h-8 items-center gap-1 rounded-full border border-border bg-surface px-3 text-xs font-medium text-ink hover:bg-subtle"
                          >
                            <RotateCcw className="h-3 w-3" />
                            <span>{t('study.runner.pinClear')}</span>
                          </button>
                        )}
                      </div>
                    )}

                    {/* Free-text Input */}
                    {(activeQ.question_type === 'short_answer' ||
                      activeQ.question_type === 'vocab' ||
                      activeQ.question_type === 'image_qa' ||
                      activeQ.question_type === 'image_occlusion' ||
                      activeQ.question_type === 'pin_identify') && (
                      <div className="space-y-1.5">
                        <label className="block text-xs font-medium text-ink-muted">
                          {activeQ.question_type === 'pin_identify'
                            ? t('study.runner.pinIdentifyPrompt')
                            : t('study.runner.typeAnswer')}
                        </label>
                        <input
                          type="text"
                          disabled={locked}
                          value={draft.textAnswer}
                          onChange={(e) =>
                            updateDraft(sessionIndex, (prev) => ({
                              ...prev,
                              textAnswer: e.target.value,
                            }))
                          }
                          placeholder={t('study.runner.typeAnswer')}
                          className="h-12 w-full rounded-2xl border border-border bg-canvas px-4 text-sm text-ink placeholder:text-ink-muted disabled:opacity-65"
                        />
                      </div>
                    )}

                    {/* Immediate Grading Feedback Banner */}
                    {fb?.grading && (
                      <div
                        className={`space-y-3 rounded-2xl border p-4 ${
                          fb.grading.voided
                            ? 'border-border bg-subtle'
                            : fb.grading.is_correct
                              ? 'border-ok/40 bg-ok-soft/60'
                              : 'border-bad/40 bg-bad-soft/60'
                        }`}
                      >
                        <div className="flex items-center gap-2 text-sm font-semibold">
                          {fb.grading.voided ? (
                            <Info className="h-4 w-4 text-ink-muted" />
                          ) : fb.grading.is_correct ? (
                            <CheckCircle2 className="h-4 w-4 text-ok" />
                          ) : (
                            <XCircle className="h-4 w-4 text-bad" />
                          )}
                          <span
                            className={
                              fb.grading.voided
                                ? 'text-ink'
                                : fb.grading.is_correct
                                  ? 'text-ok'
                                  : 'text-bad'
                            }
                          >
                            {fb.timedOut
                              ? t('study.runner.grading.timedOut')
                              : fb.grading.voided
                                ? t('study.runner.grading.voided')
                                : fb.grading.is_correct
                                  ? t('study.runner.grading.correct')
                                  : t('study.runner.grading.incorrect')}
                          </span>
                        </div>

                        {activeQ.answer && activeQ.answer.length > 0 && (
                          <div className="text-xs text-ink">
                            <span className="font-semibold">
                              {t('study.runner.grading.officialAnswer')}:{' '}
                            </span>
                            {activeQ.answer.join(', ')}
                          </div>
                        )}

                        {activeQ.answer_basis && (
                          <div className="text-xs text-ink-muted">
                            <span className="font-semibold text-ink">
                              {t('study.runner.grading.answerBasis')}:{' '}
                            </span>
                            {activeQ.answer_basis}
                          </div>
                        )}

                        {activeQ.explanation_blocks && activeQ.explanation_blocks.length > 0 && (
                          <div className="space-y-2 border-t border-border/60 pt-2.5">
                            <div className="text-xs font-semibold text-ink-muted">
                              {t('study.runner.grading.explanation')}
                            </div>
                            {activeQ.explanation_blocks.map((block, eIdx) => (
                              <BlockRenderer key={eIdx} block={block} />
                            ))}
                          </div>
                        )}

                        {activeQ.anchor_ids.length > 0 && (
                          <div className="flex flex-wrap gap-1.5 pt-1">
                            {activeQ.anchor_ids.map((aid, aIdx) => (
                              <button
                                key={aid}
                                type="button"
                                onClick={() => setActiveAnchorId(aid)}
                                className="inline-flex h-8 items-center gap-1.5 rounded-full border border-border bg-surface px-3 text-xs font-medium text-accent hover:border-accent"
                              >
                                <FileSearch className="h-3.5 w-3.5" />
                                <span>
                                  {t('reading.sourceAnchor')} #{aIdx + 1}
                                </span>
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </article>
                )
              })}
            </div>
          ) : null}
        </div>
      </div>

      {/* FIXED Bottom Navigation & 5-Level Confidence Bar (Sketch 1 + User explicit rule) */}
      <footer className="pb-safe shrink-0 border-t border-border bg-surface px-4 py-3 sm:px-6">
        <div className="mx-auto flex max-w-3xl flex-col gap-2.5">
          {/* Five tactile confidence pills from Sketch 1: 確定 / 還可以 / 猶豫中 / 不熟 / 亂猜 */}
          <div
            className="flex flex-wrap items-center justify-center gap-2"
            role="radiogroup"
            aria-label={t('study.runner.confidence')}
          >
            {CONFIDENCE_LEVELS_5.map((level) => {
              const active = confidence5 === level
              return (
                <button
                  key={level}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  onClick={() => setConfidence5(level)}
                  className={`h-9 min-w-[68px] rounded-full border px-4 text-xs font-medium transition-all ${
                    active
                      ? 'border-accent bg-accent-soft font-semibold text-accent shadow-2xs'
                      : 'border-border bg-canvas/70 text-ink-muted hover:border-accent/50 hover:text-ink'
                  }`}
                >
                  {t(`workspace.confidence5.${level}`)}
                </button>
              )
            })}
          </div>

          {/* Prev / Submit / Next row */}
          <div className="flex items-center justify-between gap-2">
            <button
              type="button"
              disabled={!hasPrevUnit}
              onClick={() => void navigateToUnit(clampedUnitIdx - 1)}
              className="inline-flex h-10 items-center gap-1 rounded-full border border-border bg-canvas px-4 text-xs font-medium text-ink transition-colors hover:bg-subtle disabled:opacity-40"
            >
              <ChevronLeft className="h-4 w-4" />
              <span>{t('workspace.sidebar.prevQuestion')}</span>
            </button>

            <button
              type="button"
              disabled={submittableItems.length === 0 || submitMutation.isPending}
              onClick={() => void handleSubmitCurrentUnit()}
              className="inline-flex h-10 flex-1 max-w-xs items-center justify-center gap-1.5 rounded-full bg-accent px-5 text-xs font-semibold text-white transition-colors hover:bg-accent-hover disabled:opacity-40"
            >
              <Send className="h-3.5 w-3.5" />
              <span>
                {currentUnit && currentUnit.items.length > 1
                  ? t('workspace.sidebar.submitGroup')
                  : t('study.runner.submitAnswer')}
              </span>
            </button>

            {hasNextUnit ? (
              <button
                type="button"
                onClick={() => void handleNextUnit()}
                className="inline-flex h-10 items-center gap-1 rounded-full border border-border bg-canvas px-4 text-xs font-medium text-ink transition-colors hover:bg-subtle"
              >
                <span>{t('workspace.sidebar.nextQuestion')}</span>
                <ChevronRight className="h-4 w-4" />
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void handleFinish()}
                disabled={finishMutation.isPending}
                className="inline-flex h-10 items-center gap-1.5 rounded-full border border-accent bg-accent-soft px-4 text-xs font-semibold text-accent hover:bg-accent hover:text-white disabled:opacity-50"
              >
                <Flag className="h-3.5 w-3.5" />
                <span>{t('study.runner.finish')}</span>
              </button>
            )}
          </div>
        </div>
      </footer>

      <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
    </div>
  )
}
