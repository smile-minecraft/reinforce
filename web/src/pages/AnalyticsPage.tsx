import { Activity, Flame, Layers, Play, Shuffle } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '../api/client'
import { useKnowledgeCards } from '../api/hooks/useKnowledgeCards'
import { useCreateSession, useWeaknessReport } from '../api/hooks/useStudySession'
import { saveSessionConfig } from '../study/sessionConfig'
import { useWorkspace } from '../workspace/workspaceContextValue'

export function AnalyticsPage() {
  const { t } = useTranslation()
  const { openTab } = useWorkspace()
  const { data: report, isLoading, error } = useWeaknessReport(20)
  const { data: cards = [] } = useKnowledgeCards()
  const createSession = useCreateSession()
  const [launchError, setLaunchError] = useState<string | null>(null)

  async function launchWeaknessDrill() {
    setLaunchError(null)
    try {
      const res = await createSession.mutateAsync({ mode: 'weakness_drill' })
      if (res.question_count === 0) {
        setLaunchError(t('study.options.emptySessionError'))
        return
      }
      const cfg = { mode: 'weakness_drill' as const, secondsPerQuestion: null, totalTimeSecs: null }
      saveSessionConfig(res.session_id, cfg)
      openTab({
        id: `session:${res.session_id}`,
        kind: 'session',
        path: `/study/session/${encodeURIComponent(res.session_id)}`,
        title: t('workspace.tabs.reviewWithTitle', {
          title: t('study.modes.weakness_drill.name'),
        }),
        navState: cfg,
      })
    } catch (err) {
      setLaunchError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  return (
    <div className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-6 sm:px-8">
      <div className="mx-auto max-w-4xl space-y-6">
        <header className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold tracking-tight text-ink">
              <Activity className="h-5 w-5 text-accent" />
              <span>{t('workspace.analytics.title')}</span>
            </h1>
            <p className="mt-1 text-sm text-ink-muted">{t('workspace.analytics.subtitle')}</p>
          </div>

          <button
            type="button"
            onClick={() => void launchWeaknessDrill()}
            disabled={createSession.isPending}
            className="inline-flex h-10 items-center gap-2 rounded-full bg-accent px-5 text-xs font-semibold text-white hover:bg-accent-hover disabled:opacity-50"
          >
            <Play className="h-3.5 w-3.5 fill-current" />
            <span>{t('workspace.analytics.startWeaknessTab')}</span>
          </button>
        </header>

        {launchError && (
          <div className="rounded-xl border border-warn/40 bg-warn-soft px-4 py-3 text-xs font-medium text-warn">
            {launchError}
          </div>
        )}

        {isLoading ? (
          <div className="py-12 text-center text-sm text-ink-muted">{t('common.loading')}</div>
        ) : error ? (
          <div className="rounded-xl bg-bad-soft p-4 text-sm text-bad">
            {error instanceof Error ? error.message : t('common.errorGeneric')}
          </div>
        ) : !report ||
          (report.knowledge_cards.length === 0 &&
            report.questions.length === 0 &&
            report.confusable_pairs.length === 0) ? (
          <div className="rounded-2xl border border-border bg-canvas/50 p-10 text-center text-sm text-ink-muted">
            {t('study.weaknessPreview.empty')}
          </div>
        ) : (
          <div className="space-y-6">
            {/* Weak Knowledge Cards */}
            <section className="space-y-3">
              <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
                <Layers className="h-4 w-4 text-accent" />
                <span>{t('workspace.analytics.weakCards')}</span>
              </h2>
              <ul className="divide-y divide-border overflow-hidden rounded-2xl border border-border bg-canvas/40">
                {report.knowledge_cards.map((entry) => {
                  const card = cards.find((c) => c.id === entry.card_id)
                  const title = card?.title ?? entry.card_id.slice(-8)
                  return (
                    <li
                      key={entry.card_id}
                      className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 text-xs"
                    >
                      <button
                        type="button"
                        onClick={() =>
                          openTab({
                            id: `card:${entry.card_id}`,
                            kind: 'card',
                            path: `/cards/${encodeURIComponent(entry.card_id)}`,
                            title,
                          })
                        }
                        className="font-semibold text-ink hover:text-accent"
                      >
                        {title}
                      </button>
                      <div className="flex flex-wrap items-center gap-3 font-mono text-ink-muted">
                        <span>{t('study.weaknessPreview.attempts', { count: entry.attempts })}</span>
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
            </section>

            {/* Weak Questions */}
            {report.questions.length > 0 && (
              <section className="space-y-3">
                <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
                  <Flame className="h-4 w-4 text-warn" />
                  <span>{t('workspace.analytics.weakQuestions')}</span>
                </h2>
                <ul className="divide-y divide-border overflow-hidden rounded-2xl border border-border bg-canvas/40">
                  {report.questions.map((entry) => (
                    <li
                      key={entry.question_id}
                      className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 text-xs"
                    >
                      <span className="font-mono text-ink">Q-{entry.question_id.slice(-8)}</span>
                      <div className="flex flex-wrap items-center gap-3 font-mono text-ink-muted">
                        <span>{t('study.weaknessPreview.attempts', { count: entry.attempts })}</span>
                        <span className="text-bad">
                          {t('study.weaknessPreview.errorRate', {
                            pct: Math.round(entry.error_rate * 100),
                          })}
                        </span>
                        <span>
                          {t('study.weaknessPreview.avgDuration', {
                            secs: (entry.average_duration_ms / 1000).toFixed(1),
                          })}
                        </span>
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {/* Confusable Option Pairs */}
            {report.confusable_pairs.length > 0 && (
              <section className="space-y-3">
                <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
                  <Shuffle className="h-4 w-4 text-accent" />
                  <span>{t('workspace.analytics.confusablePairs')}</span>
                </h2>
                <ul className="divide-y divide-border overflow-hidden rounded-2xl border border-border bg-canvas/40">
                  {report.confusable_pairs.map((pair, idx) => (
                    <li
                      key={`${pair.question_id}-${idx}`}
                      className="flex flex-wrap items-center justify-between gap-2 px-4 py-3 text-xs"
                    >
                      <span className="font-mono text-ink-muted">
                        Q-{pair.question_id.slice(-8)}
                      </span>
                      <span className="font-medium text-ink">
                        {t('workspace.analytics.wrongVsCorrect', {
                          wrong: pair.wrong_option,
                          correct: pair.correct_option,
                          count: pair.count,
                        })}
                      </span>
                    </li>
                  ))}
                </ul>
              </section>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
