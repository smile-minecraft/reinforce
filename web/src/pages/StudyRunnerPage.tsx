import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Clock,
  FileSearch,
  Flag,
  Info,
  Pause,
  Play,
  RotateCcw,
  Scale,
  X,
  XCircle,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate, useParams } from 'react-router'
import { api, ApiError } from '../api/client'
import { useQuestion } from '../api/hooks/useQuestions'
import {
  useFinishSession,
  usePauseSession,
  useResumeSession,
  useStudySession,
  useSubmitAnswer,
  useTickSession,
} from '../api/hooks/useStudySession'
import type {
  AnswerSubmission,
  Confidence,
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
import { ConfidenceAndTags } from '../study/ConfidenceAndTags'
import { OptionLinkedCards } from '../study/OptionLinkedCards'
import { PracticalCanvas } from '../study/PracticalCanvas'
import { loadSessionConfig, type StoredSessionConfig } from '../study/sessionConfig'
import { StudySummary, type SessionReviewEntry } from '../study/StudySummary'

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

export function StudyRunnerPage() {
  const { id: sessionId = '' } = useParams()
  const location = useLocation()
  const navigate = useNavigate()
  const { t } = useTranslation()

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
  const submitMutation = useSubmitAnswer(sessionId)
  const finishMutation = useFinishSession(sessionId)

  // Local draft state for the active question
  const [selectedKeys, setSelectedKeys] = useState<string[]>([])
  const [textAnswer, setTextAnswer] = useState('')
  const [pinClicks, setPinClicks] = useState<PinClick[]>([])
  const [confidence, setConfidence] = useState<Confidence>('certain')
  const [errorTags, setErrorTags] = useState<string[]>([])

  // Feedback state shown after immediate-grading submission (before advancing to next question)
  const [feedback, setFeedback] = useState<{
    question: QuestionCard
    submission: AnswerSubmission
    grading: GradingOutcome
    nextState: SessionState
    timedOut?: boolean
  } | null>(null)
  const [deferredToast, setDeferredToast] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  const [activeAnchorId, setActiveAnchorId] = useState<string | null>(null)

  // Session review history & final result
  const [history, setHistory] = useState<SessionReviewEntry[]>([])
  const [finishResult, setFinishResult] = useState<FinishSessionResponse | null>(null)

  // Visibility pause tracking
  const autoPausedByVisibilityRef = useRef(false)
  const [visibilityPausedNotice, setVisibilityPausedNotice] = useState(false)
  const [pausedForReview, setPausedForReview] = useState(false)

  // Display countdown timers (server state remains authoritative via tick)
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

  // Fetch full question details ONLY after answering (when feedback is visible) to show answer/explanations
  const { data: revealedQuestion } = useQuestion(feedback?.question.id, Boolean(feedback))

  const activeQuestion = feedback ? (revealedQuestion ?? feedback.question) : (snapshot?.question ?? null)
  const sessionState = snapshot?.state
  const isPaused = sessionState?.status === 'paused' && !pausedForReview
  const isExpired = sessionState?.status === 'expired'
  const isSubmittedOnServer = sessionState?.status === 'submitted'

  const resetDraft = useCallback(() => {
    setSelectedKeys([])
    setTextAnswer('')
    setPinClicks([])
    setConfidence('certain')
    setErrorTags([])
    setActionError(null)
  }, [])

  const handleFinish = useCallback(async () => {
    setActionError(null)
    try {
      const result = await finishMutation.mutateAsync()
      if (result.graded && result.graded.length > 0) {
        setHistory((prev) =>
          prev.map((entry) => {
            const matched = result.graded?.find(
              (g) => g.record.index === entry.index && g.record.question_id === entry.question.id,
            )
            return matched ? { ...entry, grading: matched.outcome } : entry
          }),
        )
      }
      setFinishResult(result)
      setFeedback(null)
    } catch (err) {
      setActionError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }, [finishMutation, t])

  // Sync with server via /tick
  const runServerTick = useCallback(async () => {
    if (!sessionId || tickInFlightRef.current || finishResult) return
    tickInFlightRef.current = true
    try {
      const outcome = await tickMutation.mutateAsync()
      lastSyncRef.current = Date.now()

      if (outcome.type === 'question_timed_out') {
        const timedOutQuestion = snapshot?.question
        const refreshed = await refetchSession()
        if (timedOutQuestion) {
          const entry: SessionReviewEntry = {
            index: outcome.index,
            question: timedOutQuestion,
            submission: { kind: 'timed_out' },
            confidence: 'guessed',
            errorTags: ['time_pressure'],
            grading: outcome.graded.outcome,
          }
          setHistory((prev) => [...prev, entry])
          const nextState = refreshed.data?.state ?? { status: 'submitted' as const }
          if (isPractical && nextState.status === 'in_progress') {
            setPausedForReview(true)
            await api.pauseSession(sessionId).catch(() => {})
          }
          setFeedback({
            question: timedOutQuestion,
            submission: { kind: 'timed_out' },
            grading: outcome.graded.outcome,
            nextState,
            timedOut: true,
          })
        }
      } else if (outcome.type === 'session_expired') {
        await refetchSession()
      }
    } catch {
      // Transient tick errors will retry on next interval.
    } finally {
      tickInFlightRef.current = false
    }
  }, [sessionId, finishResult, tickMutation, snapshot, refetchSession, isPractical])

  // Countdown loop for timed modes
  useEffect(() => {
    if (
      !isTimedMode ||
      !sessionState ||
      sessionState.status !== 'in_progress' ||
      Boolean(feedback) ||
      Boolean(finishResult)
    ) {
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
          if (next <= 0 && prev > 0) {
            void runServerTick()
          }
          return next
        })
      }

      if (totalExamLimitSecs !== null) {
        setTotalRemainingSecs((prev) => {
          const next = Math.max(0, prev - dt)
          if (next <= 0 && prev > 0) {
            void runServerTick()
          }
          return next
        })
      }

      if (Date.now() - lastSyncRef.current >= 5000) {
        void runServerTick()
      }
    }, 250)

    return () => window.clearInterval(timer)
  }, [
    isTimedMode,
    sessionState,
    feedback,
    finishResult,
    perQuestionLimitSecs,
    totalExamLimitSecs,
    runServerTick,
  ])

  // visibilitychange auto-pause when hidden, auto-resume when visible again
  useEffect(() => {
    if (!isTimedMode || !sessionId || finishResult) return

    function onVisibilityChange() {
      if (document.hidden) {
        if (sessionState?.status === 'in_progress' && !feedback) {
          autoPausedByVisibilityRef.current = true
          setVisibilityPausedNotice(true)
          void api
            .pauseSession(sessionId)
            .then(() => refetchSession())
            .catch(() => {})
        }
      } else if (autoPausedByVisibilityRef.current) {
        autoPausedByVisibilityRef.current = false
        void api
          .resumeSession(sessionId)
          .then(() => refetchSession())
          .catch(() => {})
      }
    }

    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => document.removeEventListener('visibilitychange', onVisibilityChange)
  }, [isTimedMode, sessionId, sessionState?.status, feedback, finishResult, refetchSession])

  // Target pin numbers for pin_locate
  const practicalMedia = useMemo(
    () => (activeQuestion ? extractPracticalMedia(activeQuestion.stem_blocks) : null),
    [activeQuestion],
  )

  const requiredPinNos = useMemo(() => {
    if (!activeQuestion || activeQuestion.question_type !== 'pin_locate') return []
    const pinNos = practicalMedia?.pins.map((p) => p.no) ?? []
    return pinNos.length > 0 ? pinNos : [1]
  }, [activeQuestion, practicalMedia])

  const nextUnplacedPinNo = useMemo(() => {
    for (const no of requiredPinNos) {
      if (!pinClicks.some((c) => c.no === no)) return no
    }
    return requiredPinNos[requiredPinNos.length - 1] ?? null
  }, [requiredPinNos, pinClicks])

  function handlePlacePin(point: NormalizedPoint) {
    if (feedback || nextUnplacedPinNo === null) return
    setPinClicks((prev) => {
      const targetNo =
        requiredPinNos.find((no) => !prev.some((c) => c.no === no)) ??
        requiredPinNos[requiredPinNos.length - 1]
      const filtered = prev.filter((c) => c.no !== targetNo)
      return [...filtered, { no: targetNo, point }]
    })
  }

  function buildSubmission(): AnswerSubmission | null {
    if (!activeQuestion) return null
    if (activeQuestion.grading_mode === 'voided') {
      return activeQuestion.options.length > 0
        ? { kind: 'keys', values: selectedKeys.length > 0 ? selectedKeys : [activeQuestion.options[0].key] }
        : { kind: 'text', value: textAnswer.trim() || '-' }
    }

    switch (activeQuestion.question_type) {
      case 'single_choice':
      case 'multiple_choice':
        return selectedKeys.length > 0 ? { kind: 'keys', values: selectedKeys } : null
      case 'pin_locate':
        return pinClicks.length > 0 ? { kind: 'pin_clicks', clicks: pinClicks } : null
      case 'short_answer':
      case 'vocab':
      case 'image_qa':
      case 'image_occlusion':
      case 'pin_identify':
        return textAnswer.trim().length > 0 ? { kind: 'text', value: textAnswer.trim() } : null
    }
  }

  async function handleSubmitAnswer() {
    if (!snapshot?.question || !sessionState || sessionState.status !== 'in_progress') return
    const submission = buildSubmission()
    if (!submission) return

    setActionError(null)
    const currentQ = snapshot.question
    const currentIndex = sessionState.index

    try {
      const res = await submitMutation.mutateAsync({
        submission,
        confidence,
        error_tags: errorTags.length > 0 ? errorTags : undefined,
      })

      const entry: SessionReviewEntry = {
        index: currentIndex,
        question: currentQ,
        submission,
        confidence,
        errorTags: [...errorTags],
        grading: res.grading,
      }
      setHistory((prev) => [...prev, entry])

      if (res.grading === null) {
        // Deferred grading (full_exam): no right/wrong feedback until finish
        resetDraft()
        setDeferredToast(true)
        window.setTimeout(() => setDeferredToast(false), 1600)
        await refetchSession()
      } else {
        // Immediate grading: pause practical timer while user reviews feedback
        if (isPractical && res.state.status === 'in_progress') {
          setPausedForReview(true)
          await api.pauseSession(sessionId).catch(() => {})
        }
        setFeedback({
          question: currentQ,
          submission,
          grading: res.grading,
          nextState: res.state,
        })
      }
    } catch (err) {
      setActionError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  async function handleNextAfterFeedback() {
    if (!feedback) return
    const nextState = feedback.nextState
    setFeedback(null)
    resetDraft()
    if (perQuestionLimitSecs !== null) {
      setQuestionRemainingSecs(perQuestionLimitSecs)
    }

    if (nextState.status === 'submitted') {
      setPausedForReview(false)
      await handleFinish()
      return
    }

    if (pausedForReview) {
      setPausedForReview(false)
      await api.resumeSession(sessionId).catch(() => {})
    }
    await refetchSession()
  }

  function toggleErrorTag(tag: string) {
    setErrorTags((prev) =>
      prev.includes(tag) ? prev.filter((item) => item !== tag) : [...prev, tag],
    )
    if (feedback) {
      setHistory((prev) =>
        prev.map((item, idx) =>
          idx === prev.length - 1
            ? {
                ...item,
                errorTags: item.errorTags.includes(tag)
                  ? item.errorTags.filter((itemTag) => itemTag !== tag)
                  : [...item.errorTags, tag],
              }
            : item,
        ),
      )
    }
  }

  // Loading & Error screens
  if (isLoading) {
    return (
      <div className="flex min-h-dvh items-center justify-center text-sm text-ink-muted">
        {t('common.loading')}
      </div>
    )
  }

  if (sessionError || !snapshot) {
    return (
      <div className="mx-auto max-w-xl px-4 py-12">
        <div className="rounded-xl bg-bad-soft p-5 text-sm text-bad">
          {sessionError instanceof Error ? sessionError.message : t('common.errorGeneric')}
        </div>
        <button
          type="button"
          onClick={() => void navigate('/study')}
          className="mt-4 inline-flex min-h-11 items-center rounded-lg border border-border bg-surface px-4 text-sm font-medium text-ink"
        >
          {t('study.runner.summary.backToStudy')}
        </button>
      </div>
    )
  }

  // Summary screen
  if (finishResult) {
    return (
      <>
        <StudySummary
          score={finishResult.score}
          attemptsWritten={finishResult.attempts_written}
          entries={history}
          onOpenAnchor={setActiveAnchorId}
        />
        <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
      </>
    )
  }

  const currentIdx =
    sessionState?.status === 'in_progress' ||
    sessionState?.status === 'paused' ||
    sessionState?.status === 'expired'
      ? sessionState.index
      : snapshot.question_count
  const displayIndex = feedback ? history[history.length - 1]?.index ?? currentIdx : currentIdx
  const progressPct =
    snapshot.question_count > 0
      ? Math.min(100, ((displayIndex + (feedback ? 1 : 0)) / snapshot.question_count) * 100)
      : 100

  const showPracticalCanvas = Boolean(
    activeQuestion &&
      practicalMedia &&
      (activeQuestion.question_type === 'pin_locate' ||
        activeQuestion.question_type === 'pin_identify' ||
        activeQuestion.question_type === 'image_qa'),
  )

  const nonCanvasBlocks = activeQuestion
    ? activeQuestion.stem_blocks.filter((b) => {
        if (!showPracticalCanvas) return true
        return b.type !== 'interactive_pins' && b.type !== 'image_occlusion'
      })
    : []

  const canSubmit = !feedback && buildSubmission() !== null && sessionState?.status === 'in_progress'
  const timerWarning =
    (perQuestionLimitSecs !== null && questionRemainingSecs <= 5) ||
    (totalExamLimitSecs !== null && totalRemainingSecs <= 60)

  return (
    <div className="flex min-h-dvh flex-col bg-canvas">
      {/* Top Runner Header */}
      <header className="sticky top-0 z-30 border-b border-border bg-surface/95 backdrop-blur">
        <div className="mx-auto flex max-w-4xl items-center justify-between gap-2 px-3 py-2 sm:px-6">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => void navigate('/study')}
              aria-label={t('study.runner.exit')}
              className="flex min-h-11 min-w-11 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink"
            >
              <X className="h-5 w-5" />
            </button>
            <div>
              <div className="text-xs font-medium text-ink-muted">
                {t(`study.modes.${mode}.name`)}
              </div>
              <div className="text-sm font-semibold text-ink">
                {t('study.runner.progress', {
                  current: Math.min(displayIndex + 1, Math.max(1, snapshot.question_count)),
                  total: snapshot.question_count,
                })}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            {/* Timer Pill */}
            {perQuestionLimitSecs !== null && (
              <div
                className={`inline-flex min-h-10 items-center gap-1.5 rounded-lg border px-3 font-mono text-sm font-bold tabular-nums ${
                  timerWarning
                    ? 'border-bad bg-bad-soft text-bad'
                    : 'border-border bg-subtle text-ink'
                }`}
              >
                <Clock className="h-4 w-4" />
                <span>{formatTimer(questionRemainingSecs)}</span>
              </div>
            )}

            {totalExamLimitSecs !== null && (
              <div
                className={`inline-flex min-h-10 items-center gap-1.5 rounded-lg border px-3 font-mono text-sm font-bold tabular-nums ${
                  timerWarning
                    ? 'border-bad bg-bad-soft text-bad'
                    : 'border-border bg-subtle text-ink'
                }`}
              >
                <Clock className="h-4 w-4" />
                <span>{formatTimer(totalRemainingSecs)}</span>
              </div>
            )}

            {/* Pause / Resume */}
            {!feedback && (sessionState?.status === 'in_progress' || isPaused) && (
              <button
                type="button"
                onClick={() =>
                  isPaused ? resumeMutation.mutate() : pauseMutation.mutate()
                }
                disabled={pauseMutation.isPending || resumeMutation.isPending}
                className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
              >
                {isPaused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
                <span className="hidden sm:inline">
                  {isPaused ? t('study.runner.resume') : t('study.runner.pause')}
                </span>
              </button>
            )}

            {/* Finish Button */}
            <button
              type="button"
              onClick={() => void handleFinish()}
              disabled={finishMutation.isPending}
              className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
            >
              <Flag className="h-3.5 w-3.5 text-accent" />
              <span>{t('study.runner.finish')}</span>
            </button>
          </div>
        </div>

        {/* Thin progress line */}
        <div className="h-1 w-full bg-subtle">
          <div
            className="h-full bg-accent transition-all duration-200"
            style={{ width: `${progressPct}%` }}
          />
        </div>
      </header>

      {/* Main Runner Body */}
      <main className="mx-auto flex w-full max-w-4xl flex-1 flex-col gap-4 px-4 py-5 sm:px-6">
        {isFullExam && !isSubmittedOnServer && (
          <div className="rounded-lg border border-accent/30 bg-accent-soft px-3.5 py-2 text-xs text-accent">
            {t('study.runner.deferredBanner')}
          </div>
        )}

        {visibilityPausedNotice && (
          <div className="flex items-center justify-between gap-2 rounded-lg border border-warn/40 bg-warn-soft px-3.5 py-2.5 text-xs text-warn">
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

        {deferredToast && (
          <div className="rounded-lg bg-ok-soft px-3.5 py-2 text-xs font-medium text-ok">
            {t('study.runner.recorded')}
          </div>
        )}

        {actionError && (
          <div className="rounded-lg bg-bad-soft px-4 py-3 text-sm text-bad">{actionError}</div>
        )}

        {/* Manual Pause Overlay */}
        {isPaused && (
          <section className="rounded-2xl border border-border bg-surface p-8 text-center">
            <Pause className="mx-auto h-10 w-10 text-accent" />
            <h2 className="mt-3 text-lg font-semibold text-ink">{t('study.runner.pausedTitle')}</h2>
            <p className="mt-1 text-sm text-ink-muted">{t('study.runner.pausedManual')}</p>
            <button
              type="button"
              onClick={() => resumeMutation.mutate()}
              className="mt-5 inline-flex min-h-11 items-center gap-2 rounded-xl bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
            >
              <Play className="h-4 w-4 fill-current" />
              <span>{t('study.runner.resume')}</span>
            </button>
          </section>
        )}

        {/* Expired State */}
        {isExpired && (
          <section className="rounded-2xl border border-warn/40 bg-surface p-8 text-center">
            <AlertTriangle className="mx-auto h-10 w-10 text-warn" />
            <h2 className="mt-3 text-lg font-semibold text-ink">{t('study.runner.expiredTitle')}</h2>
            <p className="mt-1 text-sm text-ink-muted">{t('study.runner.expiredDesc')}</p>
            <button
              type="button"
              onClick={() => void handleFinish()}
              disabled={finishMutation.isPending}
              className="mt-5 inline-flex min-h-11 items-center gap-2 rounded-xl bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
            >
              <Flag className="h-4 w-4" />
              <span>{t('study.runner.finish')}</span>
            </button>
          </section>
        )}

        {/* All questions answered on server (deferred exam ready to finish) */}
        {isSubmittedOnServer && !feedback && (
          <section className="rounded-2xl border border-border bg-surface p-8 text-center">
            <CheckCircle2 className="mx-auto h-10 w-10 text-ok" />
            <h2 className="mt-3 text-lg font-semibold text-ink">
              {t('study.runner.summary.title')}
            </h2>
            <button
              type="button"
              onClick={() => void handleFinish()}
              disabled={finishMutation.isPending}
              className="mt-5 inline-flex min-h-12 items-center gap-2 rounded-xl bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
            >
              <Flag className="h-4 w-4" />
              <span>{t('study.runner.finish')}</span>
            </button>
          </section>
        )}

        {/* Active Question Stage */}
        {!isPaused && !isExpired && activeQuestion && (!isSubmittedOnServer || feedback) && (
          <div className="space-y-4">
            {/* Practical / Specimen Zoom Canvas (55svh portrait, 65svh landscape) */}
            {showPracticalCanvas && practicalMedia && (
              <PracticalCanvas
                resourceId={practicalMedia.resourceId}
                overlayResourceId={practicalMedia.overlayResourceId}
                pins={practicalMedia.pins}
                mode={
                  activeQuestion.question_type === 'pin_locate'
                    ? 'locate'
                    : activeQuestion.question_type === 'pin_identify'
                      ? 'identify'
                      : 'view'
                }
                placedClicks={pinClicks}
                activePinNo={nextUnplacedPinNo}
                revealPins={Boolean(feedback)}
                onPlacePin={handlePlacePin}
              />
            )}

            {/* Question Stem Card */}
            <section className="rounded-xl border border-border bg-surface p-5">
              <div className="mb-3 flex flex-wrap items-center gap-2 text-xs">
                <span className="rounded bg-subtle px-2 py-0.5 font-medium text-ink-muted">
                  {t(`study.runner.questionTypes.${activeQuestion.question_type}`)}
                </span>
                <span className="rounded bg-subtle px-2 py-0.5 text-ink-muted">
                  {activeQuestion.subject}
                </span>
                {activeQuestion.grading_mode === 'any_accepted' && (
                  <span className="inline-flex items-center gap-1 rounded bg-warn-soft px-2 py-0.5 font-medium text-warn">
                    <Scale className="h-3 w-3" />
                    {t('study.runner.grading.anyAccepted')}
                  </span>
                )}
                {activeQuestion.grading_mode === 'voided' && (
                  <span className="inline-flex items-center gap-1 rounded bg-subtle px-2 py-0.5 font-medium text-ink-muted">
                    <Info className="h-3 w-3" />
                    {t('study.runner.grading.voided')}
                  </span>
                )}
              </div>

              <div className="space-y-3">
                {nonCanvasBlocks.map((block, idx) => (
                  <BlockRenderer
                    key={`${activeQuestion.id}-stem-${idx}`}
                    block={block}
                    hidePinTerms={!feedback}
                  />
                ))}
              </div>
            </section>

            {/* Answer Input Controls */}
            {activeQuestion.options.length > 0 &&
              (activeQuestion.question_type === 'single_choice' ||
                activeQuestion.question_type === 'multiple_choice') && (
                <section className="space-y-2.5">
                  {activeQuestion.options.map((opt) => {
                    const isSelected = selectedKeys.includes(opt.key)
                    const acceptedKeys = new Set<string>([
                      ...(activeQuestion.answer ?? []),
                      ...(activeQuestion.grading_mode === 'any_accepted'
                        ? (activeQuestion.accepted_answers ?? []).flat()
                        : []),
                    ])
                    const isCorrectOpt = Boolean(feedback && acceptedKeys.has(opt.key))
                    const isWrongSelected = Boolean(
                      feedback &&
                        isSelected &&
                        !feedback.grading.voided &&
                        acceptedKeys.size > 0 &&
                        !acceptedKeys.has(opt.key),
                    )

                    let borderClass = 'border-border bg-surface hover:border-accent/60'
                    if (feedback) {
                      if (isCorrectOpt) borderClass = 'border-ok bg-ok-soft/60'
                      else if (isWrongSelected) borderClass = 'border-bad bg-bad-soft/60'
                      else borderClass = 'border-border bg-surface'
                    } else if (isSelected) {
                      borderClass = 'border-accent bg-accent-soft'
                    }

                    return (
                      <div
                        key={opt.key}
                        className={`rounded-xl border p-3.5 transition-colors ${borderClass}`}
                      >
                        <button
                          type="button"
                          disabled={Boolean(feedback)}
                          onClick={() => {
                            if (activeQuestion.question_type === 'single_choice') {
                              setSelectedKeys([opt.key])
                            } else {
                              setSelectedKeys((prev) =>
                                prev.includes(opt.key)
                                  ? prev.filter((k) => k !== opt.key)
                                  : [...prev, opt.key],
                              )
                            }
                          }}
                          aria-pressed={isSelected}
                          className="flex min-h-11 w-full items-start gap-3 text-left"
                        >
                          <span
                            className={`mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border font-mono text-xs font-bold ${
                              isSelected
                                ? 'border-accent bg-accent text-white'
                                : 'border-border bg-canvas text-ink-muted'
                            }`}
                          >
                            {opt.key}
                          </span>
                          <span className="flex-1 pt-0.5 text-sm font-medium text-ink">
                            {opt.content}
                          </span>
                        </button>

                        {/* Post-answer option explanation + expandable linked Knowledge Cards */}
                        {feedback && (
                          <div className="mt-2 pl-10">
                            {opt.explanation && (
                              <p className="text-xs text-ink-muted">{opt.explanation}</p>
                            )}
                            <OptionLinkedCards
                              cardIds={opt.linked_card_ids}
                              onOpenAnchor={setActiveAnchorId}
                            />
                          </div>
                        )}
                      </div>
                    )
                  })}
                </section>
              )}

            {/* Pin Locate Controls */}
            {activeQuestion.question_type === 'pin_locate' && !feedback && (
              <section className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-surface p-4">
                <div className="text-sm text-ink">
                  {pinClicks.length >= requiredPinNos.length
                    ? t('study.runner.pinLocateAllPlaced')
                    : t('study.runner.pinLocatePrompt', { no: nextUnplacedPinNo ?? 1 })}
                </div>
                {pinClicks.length > 0 && (
                  <button
                    type="button"
                    onClick={() => setPinClicks([])}
                    className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
                  >
                    <RotateCcw className="h-3.5 w-3.5" />
                    <span>{t('study.runner.pinClear')}</span>
                  </button>
                )}
              </section>
            )}

            {/* Free-text Input (short_answer / vocab / image_qa / pin_identify / image_occlusion) */}
            {(activeQuestion.question_type === 'short_answer' ||
              activeQuestion.question_type === 'vocab' ||
              activeQuestion.question_type === 'image_qa' ||
              activeQuestion.question_type === 'image_occlusion' ||
              activeQuestion.question_type === 'pin_identify') && (
              <section className="rounded-xl border border-border bg-surface p-4">
                <label className="block">
                  <span className="text-xs font-medium text-ink-muted">
                    {activeQuestion.question_type === 'pin_identify'
                      ? t('study.runner.pinIdentifyPrompt')
                      : t('study.runner.typeAnswer')}
                  </span>
                  <input
                    type="text"
                    disabled={Boolean(feedback)}
                    value={textAnswer}
                    onChange={(e) => setTextAnswer(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && canSubmit) {
                        void handleSubmitAnswer()
                      }
                    }}
                    placeholder={t('study.runner.typeAnswer')}
                    className="mt-1.5 min-h-12 w-full rounded-lg border border-border bg-canvas px-3.5 text-base text-ink disabled:opacity-60"
                  />
                </label>
              </section>
            )}

            {/* Immediate Grading Feedback Banner */}
            {feedback && (
              <section
                className={`space-y-3 rounded-xl border p-4 ${
                  feedback.grading.voided
                    ? 'border-border bg-subtle'
                    : feedback.grading.is_correct
                      ? 'border-ok/40 bg-ok-soft'
                      : 'border-bad/40 bg-bad-soft'
                }`}
              >
                <div className="flex flex-wrap items-center gap-2">
                  {feedback.grading.voided ? (
                    <Info className="h-5 w-5 text-ink-muted" />
                  ) : feedback.grading.is_correct ? (
                    <CheckCircle2 className="h-5 w-5 text-ok" />
                  ) : (
                    <XCircle className="h-5 w-5 text-bad" />
                  )}
                  <span
                    className={`text-base font-semibold ${
                      feedback.grading.voided
                        ? 'text-ink'
                        : feedback.grading.is_correct
                          ? 'text-ok'
                          : 'text-bad'
                    }`}
                  >
                    {feedback.timedOut
                      ? t('study.runner.grading.timedOut')
                      : feedback.grading.voided
                        ? t('study.runner.grading.voided')
                        : feedback.grading.is_correct
                          ? t('study.runner.grading.correct')
                          : t('study.runner.grading.incorrect')}
                  </span>

                  {!feedback.grading.voided &&
                    activeQuestion.grading_mode === 'any_accepted' && (
                      <span className="inline-flex items-center gap-1 rounded-full bg-warn-soft px-2.5 py-0.5 text-xs font-medium text-warn">
                        <Scale className="h-3.5 w-3.5" />
                        {t('study.runner.grading.anyAccepted')}
                      </span>
                    )}
                </div>

                {activeQuestion.answer && activeQuestion.answer.length > 0 && (
                  <div className="text-sm text-ink">
                    <span className="font-semibold">
                      {t('study.runner.grading.officialAnswer')}:{' '}
                    </span>
                    {activeQuestion.answer.join(', ')}
                    {activeQuestion.grading_mode === 'any_accepted' &&
                      activeQuestion.accepted_answers &&
                      activeQuestion.accepted_answers.length > 0 && (
                        <span className="ml-2 text-xs text-ink-muted">
                          ({activeQuestion.accepted_answers.map((g) => g.join('+')).join(' / ')})
                        </span>
                      )}
                  </div>
                )}

                {activeQuestion.answer_basis && (
                  <p className="text-xs text-ink-muted">
                    <span className="font-semibold text-ink">
                      {t('study.runner.grading.answerBasis')}:{' '}
                    </span>
                    {activeQuestion.answer_basis}
                  </p>
                )}

                {activeQuestion.explanation_blocks &&
                  activeQuestion.explanation_blocks.length > 0 && (
                    <div className="space-y-2 border-t border-border/60 pt-3">
                      <div className="text-xs font-semibold text-ink-muted">
                        {t('study.runner.grading.explanation')}
                      </div>
                      {activeQuestion.explanation_blocks.map((block, i) => (
                        <BlockRenderer key={i} block={block} />
                      ))}
                    </div>
                  )}

                {activeQuestion.anchor_ids.length > 0 && (
                  <div className="flex flex-wrap gap-2 pt-1">
                    {activeQuestion.anchor_ids.map((aid, idx) => (
                      <button
                        key={aid}
                        type="button"
                        onClick={() => setActiveAnchorId(aid)}
                        className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-surface px-3 text-xs font-medium text-accent hover:bg-accent-soft"
                      >
                        <FileSearch className="h-3.5 w-3.5" />
                        <span>
                          {t('reading.sourceAnchor')} #{idx + 1}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
              </section>
            )}

            {/* Confidence & Error Tags */}
            <ConfidenceAndTags
              confidence={confidence}
              onConfidenceChange={setConfidence}
              errorTags={errorTags}
              onToggleErrorTag={toggleErrorTag}
              confidenceDisabled={feedback !== null}
              showErrorTags
            />

            {/* Primary Action Button */}
            <div className="pb-safe pt-1">
              {feedback ? (
                <button
                  type="button"
                  onClick={() => void handleNextAfterFeedback()}
                  className="flex min-h-12 w-full items-center justify-center gap-2 rounded-xl bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover"
                >
                  <span>
                    {feedback.nextState.status === 'submitted'
                      ? t('study.runner.viewSummary')
                      : t('study.runner.nextQuestion')}
                  </span>
                  <ArrowRight className="h-4 w-4" />
                </button>
              ) : (
                <button
                  type="button"
                  disabled={!canSubmit || submitMutation.isPending}
                  onClick={() => void handleSubmitAnswer()}
                  className="flex min-h-12 w-full items-center justify-center gap-2 rounded-xl bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover disabled:opacity-40"
                >
                  <span>{t('study.runner.submitAnswer')}</span>
                </button>
              )}
            </div>
          </div>
        )}
      </main>

      <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
    </div>
  )
}
