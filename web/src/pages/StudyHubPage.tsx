import {
  CheckSquare,
  Clock,
  FileSpreadsheet,
  Flame,
  FolderOpen,
  Layers,
  Play,
  Sparkles,
  Timer,
} from 'lucide-react'
import { useMemo, useState, type ComponentType } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '../api/client'
import { useKnowledgeCards } from '../api/hooks/useKnowledgeCards'
import { useNotes } from '../api/hooks/useNotes'
import { useExamSets } from '../api/hooks/useQuestions'
import { useCreateSession, useWeaknessReport } from '../api/hooks/useStudySession'
import { useSubjectGroups } from '../api/hooks/useSubjectGroups'
import type { CreateSessionRequest, SessionMode } from '../api/types'
import { SubjectGroupTree } from '../components/SubjectGroupTree'
import { saveSessionConfig } from '../study/sessionConfig'
import { RightSidebarSlot } from '../workspace/WorkspaceContext'
import { useWorkspace } from '../workspace/workspaceContextValue'
import { resolveGroupSubjects } from '../workspace/workspaceLogic'

const MODES: Array<{ id: SessionMode; icon: ComponentType<{ className?: string }> }> = [
  { id: 'daily_due', icon: Sparkles },
  { id: 'full_exam', icon: FileSpreadsheet },
  { id: 'topic_drill', icon: Layers },
  { id: 'practical_exam', icon: Timer },
  { id: 'weakness_drill', icon: Flame },
]

const PRACTICAL_PRESETS = [15, 30, 45, 60, 120]
const PIN_RADII = [0.04, 0.06, 0.08]

export function StudyHubPage({ defaultMode }: { defaultMode?: SessionMode }) {
  const { t } = useTranslation()
  const { openTab } = useWorkspace()

  const [mode, setMode] = useState<SessionMode>(defaultMode ?? 'daily_due')
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null)
  const [questionLimit, setQuestionLimit] = useState(20)
  const [newCardLimit, setNewCardLimit] = useState(10)
  const [selectedExamSetId, setSelectedExamSetId] = useState('')
  const [totalTimeMins, setTotalTimeMins] = useState('')
  const [selectedCardIds, setSelectedCardIds] = useState<string[]>([])
  const [secondsPerQuestion, setSecondsPerQuestion] = useState(30)
  const [pinRadius, setPinRadius] = useState(0.06)
  const [affectsScheduling, setAffectsScheduling] = useState(true)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  const { data: examSets = [], isLoading: loadingExams } = useExamSets()
  const { data: cards = [], isLoading: loadingCards } = useKnowledgeCards()
  const { data: notes = [] } = useNotes()
  const { data: subjectGroups = [] } = useSubjectGroups()
  const { data: weakness, isLoading: loadingWeakness } = useWeaknessReport(10)
  const createSession = useCreateSession()

  const availableSubjects = useMemo(() => {
    const set = new Set<string>()
    for (const n of notes) set.add(n.subject)
    for (const c of cards) set.add(c.subject)
    for (const e of examSets) set.add(e.subject)
    return Array.from(set).sort()
  }, [notes, cards, examSets])

  const activeGroup = useMemo(
    () => (selectedGroupId ? subjectGroups.find((g) => g.id === selectedGroupId) ?? null : null),
    [subjectGroups, selectedGroupId],
  )

  const groupSubjects = useMemo(
    () => (selectedGroupId ? resolveGroupSubjects(subjectGroups, selectedGroupId) : null),
    [subjectGroups, selectedGroupId],
  )

  const filteredExamSets = useMemo(() => {
    if (!groupSubjects) return examSets
    const allowed = new Set(groupSubjects)
    return examSets.filter((es) => allowed.has(es.subject))
  }, [examSets, groupSubjects])

  const filteredCards = useMemo(() => {
    if (!groupSubjects) return cards
    const allowed = new Set(groupSubjects)
    return cards.filter((c) => allowed.has(c.subject))
  }, [cards, groupSubjects])

  const activeExamSetId = selectedExamSetId || (!selectedGroupId ? filteredExamSets[0]?.id || '' : '')

  function toggleCardSelection(cardId: string) {
    setSelectedCardIds((prev) =>
      prev.includes(cardId) ? prev.filter((id) => id !== cardId) : [...prev, cardId],
    )
  }

  async function handleStart() {
    setErrorMsg(null)
    const clampedSec = Math.min(300, Math.max(1, Math.round(secondsPerQuestion || 30)))
    const parsedMins = Number(totalTimeMins)
    const totalTimeSecs =
      mode === 'full_exam' && Number.isFinite(parsedMins) && parsedMins > 0
        ? Math.round(parsedMins * 60)
        : undefined

    const payload: CreateSessionRequest = {
      mode,
      subject_group_id: selectedGroupId ?? undefined,
      options: {
        question_limit: Math.min(200, Math.max(1, questionLimit)),
        affects_scheduling: affectsScheduling,
      },
    }

    if (mode === 'daily_due') {
      payload.options!.new_card_limit = Math.max(0, newCardLimit)
    } else if (mode === 'full_exam') {
      if (activeExamSetId) {
        payload.exam_set_id = activeExamSetId
      } else if (!selectedGroupId) {
        setErrorMsg(t('study.options.noExamSets'))
        return
      }
      if (totalTimeSecs) payload.options!.total_time_secs = totalTimeSecs
    } else if (mode === 'topic_drill') {
      if (selectedCardIds.length === 0) {
        setErrorMsg(t('study.options.selectCards'))
        return
      }
      payload.knowledge_card_ids = selectedCardIds
    } else if (mode === 'practical_exam') {
      payload.options!.seconds_per_question = clampedSec
      payload.options!.pin_radius = pinRadius
    }

    try {
      const res = await createSession.mutateAsync(payload)
      if (res.question_count === 0) {
        setErrorMsg(t('study.options.emptySessionError'))
        return
      }
      const cfg = {
        mode,
        secondsPerQuestion: mode === 'practical_exam' ? clampedSec : null,
        totalTimeSecs: totalTimeSecs ?? null,
      }
      saveSessionConfig(res.session_id, cfg)
      const tabTitle = activeGroup
        ? t('workspace.tabs.reviewWithTitle', { title: activeGroup.name })
        : t('workspace.tabs.reviewWithTitle', { title: t(`study.modes.${mode}.name`) })
      openTab({
        id: `session:${res.session_id}`,
        kind: 'session',
        path: `/study/session/${encodeURIComponent(res.session_id)}`,
        title: tabTitle,
        navState: cfg,
      })
    } catch (err) {
      setErrorMsg(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Right Sidebar: Pick a single nested Subject Group for the exam */}
      <RightSidebarSlot>
        <div className="rf-pane-scroll flex-1 overflow-y-auto p-3.5">
          <SubjectGroupTree
            groups={subjectGroups}
            availableSubjects={availableSubjects}
            selectedGroupId={selectedGroupId}
            onSelectGroup={(gid) => {
              setSelectedGroupId(gid)
              setSelectedExamSetId('')
              setErrorMsg(null)
            }}
            editable
          />
        </div>
      </RightSidebarSlot>

      <div className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-6 sm:px-8">
        <div className="mx-auto max-w-4xl space-y-6">
          <header className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h1 className="text-2xl font-bold tracking-tight text-ink">{t('study.title')}</h1>
              <p className="mt-1 text-sm text-ink-muted">{t('study.subtitle')}</p>
            </div>

            {activeGroup && (
              <div className="inline-flex items-center gap-2 rounded-full border border-accent/40 bg-accent-soft px-3.5 py-1.5 text-xs font-medium text-accent">
                <FolderOpen className="h-3.5 w-3.5" />
                <span>{activeGroup.name}</span>
                {groupSubjects && groupSubjects.length > 0 && (
                  <span className="opacity-75">({groupSubjects.join(', ')})</span>
                )}
              </div>
            )}
          </header>

          {/* Mode Selector Grid */}
          <div className="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-5">
            {MODES.map(({ id, icon: Icon }) => {
              const active = mode === id
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => {
                    setMode(id)
                    setErrorMsg(null)
                  }}
                  aria-pressed={active}
                  className={`flex min-h-20 flex-col items-start justify-between rounded-xl border p-3.5 text-left transition-colors ${
                    active
                      ? 'border-accent bg-accent-soft text-accent'
                      : 'border-border bg-canvas/50 text-ink hover:border-accent/50 hover:bg-surface'
                  }`}
                >
                  <Icon className="h-5 w-5" />
                  <span className="mt-2 text-sm font-semibold leading-snug">
                    {t(`study.modes.${id}.name`)}
                  </span>
                </button>
              )
            })}
          </div>

          {/* Mode Configuration Panel */}
          <section className="space-y-5 rounded-2xl border border-border bg-canvas/40 p-5 sm:p-6">
            <div>
              <h2 className="text-lg font-semibold text-ink">{t(`study.modes.${mode}.name`)}</h2>
              <p className="mt-1 text-sm text-ink-muted">{t(`study.modes.${mode}.desc`)}</p>
            </div>

            {mode === 'daily_due' && (
              <div className="grid gap-4 sm:grid-cols-2">
                <label className="block">
                  <span className="text-sm font-medium text-ink">
                    {t('study.options.questionLimit')}
                  </span>
                  <input
                    type="number"
                    min={1}
                    max={200}
                    value={questionLimit}
                    onChange={(e) => setQuestionLimit(Number(e.target.value))}
                    className="mt-1.5 h-10 w-full rounded-xl border border-border bg-surface px-3 text-sm text-ink"
                  />
                </label>
                <label className="block">
                  <span className="text-sm font-medium text-ink">
                    {t('study.options.newCardLimit')}
                  </span>
                  <input
                    type="number"
                    min={0}
                    max={100}
                    value={newCardLimit}
                    onChange={(e) => setNewCardLimit(Number(e.target.value))}
                    className="mt-1.5 h-10 w-full rounded-xl border border-border bg-surface px-3 text-sm text-ink"
                  />
                </label>
              </div>
            )}

            {mode === 'full_exam' && (
              <div className="space-y-4">
                <div className="rounded-xl border border-accent/30 bg-accent-soft px-3.5 py-2.5 text-xs text-accent">
                  {t('study.runner.deferredBanner')}
                </div>

                {loadingExams ? (
                  <div className="text-sm text-ink-muted">{t('common.loading')}</div>
                ) : filteredExamSets.length === 0 && !activeGroup ? (
                  <div className="text-sm text-ink-muted">{t('study.options.noExamSets')}</div>
                ) : (
                  <div className="grid gap-4 sm:grid-cols-2">
                    <label className="block">
                      <span className="text-sm font-medium text-ink">
                        {t('study.options.selectExamSet')}
                      </span>
                      <select
                        value={activeExamSetId}
                        onChange={(e) => setSelectedExamSetId(e.target.value)}
                        className="mt-1.5 h-10 w-full rounded-xl border border-border bg-surface px-3 text-sm text-ink"
                      >
                        {activeGroup && (
                          <option value="">
                            {t('workspace.groups.reviewThisGroup')}：{activeGroup.name}
                          </option>
                        )}
                        {filteredExamSets.map((es) => (
                          <option key={es.id} value={es.id}>
                            {es.title}
                            {es.exam_year ? ` (${es.exam_year})` : ''}
                            {es.expected_questions ? ` — ${es.expected_questions}Q` : ''}
                          </option>
                        ))}
                      </select>
                    </label>

                    <label className="block">
                      <span className="flex items-center gap-1.5 text-sm font-medium text-ink">
                        <Clock className="h-4 w-4 text-ink-muted" />
                        {t('study.options.totalTimeMins')}
                      </span>
                      <input
                        type="number"
                        min={1}
                        max={480}
                        placeholder={t('study.options.noTimeLimit')}
                        value={totalTimeMins}
                        onChange={(e) => setTotalTimeMins(e.target.value)}
                        className="mt-1.5 h-10 w-full rounded-xl border border-border bg-surface px-3 text-sm text-ink"
                      />
                    </label>
                  </div>
                )}
              </div>
            )}

            {mode === 'topic_drill' && (
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium text-ink">
                    {t('study.options.selectCards')}
                  </span>
                  <span className="text-xs font-medium text-accent">
                    {t('study.options.selectedCardsCount', { count: selectedCardIds.length })}
                  </span>
                </div>

                {loadingCards ? (
                  <div className="text-sm text-ink-muted">{t('common.loading')}</div>
                ) : filteredCards.length === 0 ? (
                  <div className="text-sm text-ink-muted">{t('reading.noCards')}</div>
                ) : (
                  <div className="grid max-h-64 gap-2 overflow-y-auto pr-1 sm:grid-cols-2">
                    {filteredCards.map((card) => {
                      const picked = selectedCardIds.includes(card.id)
                      return (
                        <button
                          key={card.id}
                          type="button"
                          onClick={() => toggleCardSelection(card.id)}
                          aria-pressed={picked}
                          className={`flex min-h-11 items-center gap-2.5 rounded-xl border px-3 py-2 text-left text-sm transition-colors ${
                            picked
                              ? 'border-accent bg-accent-soft text-accent'
                              : 'border-border bg-surface text-ink hover:bg-subtle'
                          }`}
                        >
                          <CheckSquare
                            className={`h-4 w-4 shrink-0 ${picked ? 'text-accent' : 'text-ink-muted/50'}`}
                          />
                          <span className="min-w-0 flex-1 truncate font-medium">{card.title}</span>
                        </button>
                      )
                    })}
                  </div>
                )}
              </div>
            )}

            {mode === 'practical_exam' && (
              <div className="space-y-4">
                <div>
                  <div className="flex items-center justify-between">
                    <label htmlFor="practical-secs" className="text-sm font-medium text-ink">
                      {t('study.options.secondsPerQuestion')}
                    </label>
                    <span className="font-mono text-base font-bold text-accent tabular-nums">
                      {secondsPerQuestion}s
                    </span>
                  </div>

                  <input
                    id="practical-secs"
                    type="range"
                    min={1}
                    max={300}
                    value={secondsPerQuestion}
                    onChange={(e) => setSecondsPerQuestion(Number(e.target.value))}
                    className="mt-2 h-2 w-full accent-accent"
                  />

                  <div className="mt-2.5 flex flex-wrap items-center gap-2">
                    {PRACTICAL_PRESETS.map((preset) => (
                      <button
                        key={preset}
                        type="button"
                        onClick={() => setSecondsPerQuestion(preset)}
                        className={`h-9 rounded-full border px-3 font-mono text-xs font-medium ${
                          secondsPerQuestion === preset
                            ? 'border-accent bg-accent-soft text-accent'
                            : 'border-border bg-surface text-ink-muted hover:text-ink'
                        }`}
                      >
                        {preset}s
                      </button>
                    ))}
                    <input
                      type="number"
                      min={1}
                      max={300}
                      value={secondsPerQuestion}
                      onChange={(e) => setSecondsPerQuestion(Number(e.target.value))}
                      aria-label={t('study.options.secondsPerQuestion')}
                      className="h-9 w-24 rounded-full border border-border bg-surface px-3 font-mono text-xs text-ink"
                    />
                  </div>
                </div>

                <div>
                  <span className="text-sm font-medium text-ink">{t('study.options.pinRadius')}</span>
                  <div className="mt-2 flex flex-wrap gap-2">
                    {PIN_RADII.map((r) => (
                      <button
                        key={r}
                        type="button"
                        onClick={() => setPinRadius(r)}
                        className={`h-9 rounded-full border px-3 font-mono text-xs font-medium ${
                          pinRadius === r
                            ? 'border-accent bg-accent-soft text-accent'
                            : 'border-border bg-surface text-ink-muted hover:text-ink'
                        }`}
                      >
                        ±{Math.round(r * 100)}%
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            )}

            {mode === 'weakness_drill' && (
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-ink">
                  {t('study.weaknessPreview.heading')}
                </h3>
                {loadingWeakness ? (
                  <div className="text-sm text-ink-muted">{t('common.loading')}</div>
                ) : !weakness ||
                  (weakness.knowledge_cards.length === 0 && weakness.questions.length === 0) ? (
                  <p className="rounded-xl bg-subtle p-3.5 text-sm text-ink-muted">
                    {t('study.weaknessPreview.empty')}
                  </p>
                ) : (
                  <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
                    {weakness.knowledge_cards.slice(0, 5).map((entry) => {
                      const cardTitle =
                        cards.find((c) => c.id === entry.card_id)?.title ?? entry.card_id.slice(-8)
                      return (
                        <li
                          key={entry.card_id}
                          className="flex flex-wrap items-center justify-between gap-2 px-3.5 py-2.5 text-xs"
                        >
                          <span className="font-medium text-ink">{cardTitle}</span>
                          <div className="flex flex-wrap items-center gap-2 font-mono text-ink-muted">
                            <span className="text-bad">
                              {t('study.weaknessPreview.errorRate', {
                                pct: Math.round(entry.error_rate * 100),
                              })}
                            </span>
                            <span className="text-warn">
                              {t('study.weaknessPreview.hesitantRate', {
                                pct: Math.round(entry.hesitant_rate * 100),
                              })}
                            </span>
                            <span>
                              {t('study.weaknessPreview.avgDuration', {
                                secs: (entry.average_duration_ms / 1000).toFixed(1),
                              })}
                            </span>
                          </div>
                        </li>
                      )
                    })}
                  </ul>
                )}
              </div>
            )}

            {/* Shared Footer Controls */}
            <div className="flex flex-wrap items-center justify-between gap-4 border-t border-border pt-4">
              <label className="inline-flex min-h-10 cursor-pointer items-center gap-2.5 text-sm text-ink">
                <input
                  type="checkbox"
                  checked={affectsScheduling}
                  onChange={(e) => setAffectsScheduling(e.target.checked)}
                  className="h-4 w-4 rounded accent-accent"
                />
                <span>{t('study.options.affectsScheduling')}</span>
              </label>

              <button
                type="button"
                onClick={() => void handleStart()}
                disabled={createSession.isPending}
                className="inline-flex h-11 items-center gap-2 rounded-full bg-accent px-6 text-sm font-semibold text-white hover:bg-accent-hover disabled:opacity-50"
              >
                <Play className="h-4 w-4 fill-current" />
                <span>
                  {createSession.isPending
                    ? t('study.options.starting')
                    : t('study.options.startSession')}
                </span>
              </button>
            </div>

            {errorMsg && (
              <div className="rounded-xl bg-bad-soft px-4 py-3 text-sm text-bad">{errorMsg}</div>
            )}
          </section>
        </div>
      </div>
    </div>
  )
}
