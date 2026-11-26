import { CheckCircle2, ChevronDown, ChevronUp, Info, Scale, XCircle } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import type {
  AnswerSubmission,
  Confidence,
  GradingOutcome,
  QuestionCard,
  Score,
} from '../api/types'
import { BlockRenderer } from '../blocks/BlockRenderer'
import { OptionLinkedCards } from './OptionLinkedCards'

export interface SessionReviewEntry {
  index: number
  question: QuestionCard
  submission: AnswerSubmission
  confidence: Confidence
  errorTags: string[]
  grading: GradingOutcome | null
}

function formatSubmission(submission: AnswerSubmission): string {
  switch (submission.kind) {
    case 'keys':
      return submission.values.join(', ')
    case 'text':
      return submission.value
    case 'pin_clicks':
      return submission.clicks
        .map((c) => `#${c.no} (${c.point.x.toFixed(2)}, ${c.point.y.toFixed(2)})`)
        .join(', ')
    case 'timed_out':
      return '—'
  }
}

function ReviewEntryItem({
  entry,
  onOpenAnchor,
}: {
  entry: SessionReviewEntry
  onOpenAnchor: (anchorId: string) => void
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const question = entry.question

  const outcome = entry.grading
  const isVoided = outcome?.voided || question.grading_mode === 'voided'
  const isAnyAccepted = !isVoided && question.grading_mode === 'any_accepted' && outcome?.is_correct

  return (
    <div className="rounded-xl border border-border bg-surface">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex min-h-14 w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-subtle/50"
      >
        <div className="flex min-w-0 items-center gap-3">
          <span className="font-mono text-xs font-semibold text-ink-muted">
            #{entry.index + 1}
          </span>

          {isVoided ? (
            <Info className="h-4 w-4 shrink-0 text-ink-muted" />
          ) : outcome?.is_correct ? (
            <CheckCircle2 className="h-4 w-4 shrink-0 text-ok" />
          ) : (
            <XCircle className="h-4 w-4 shrink-0 text-bad" />
          )}

          <div className="min-w-0">
            <div className="truncate text-sm font-medium text-ink">
              {question.stem_blocks
                .map((b) => (b.type === 'markdown' ? b.text : ''))
                .join(' ')
                .slice(0, 90) || t(`study.runner.questionTypes.${question.question_type}`)}
            </div>
            <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-ink-muted">
              <span>
                {t('study.runner.summary.yourAnswer')}: {formatSubmission(entry.submission)}
              </span>
              <span>{t(`study.runner.confidenceLevels.${entry.confidence}`)}</span>
              {isAnyAccepted && (
                <span className="inline-flex items-center gap-1 rounded bg-warn-soft px-1.5 py-0.5 font-medium text-warn">
                  <Scale className="h-3 w-3" />
                  {t('study.runner.grading.anyAccepted')}
                </span>
              )}
              {isVoided && (
                <span className="rounded bg-subtle px-1.5 py-0.5 font-medium text-ink-muted">
                  {t('study.runner.grading.voided')}
                </span>
              )}
            </div>
          </div>
        </div>

        {open ? (
          <ChevronUp className="h-4 w-4 shrink-0 text-ink-muted" />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0 text-ink-muted" />
        )}
      </button>

      {open && (
        <div className="space-y-4 border-t border-border p-4">
          <div className="space-y-2">
            {question.stem_blocks.map((block, i) => (
              <BlockRenderer key={i} block={block} />
            ))}
          </div>

          {question.answer && question.answer.length > 0 && (
            <div className="rounded-lg bg-ok-soft px-3 py-2 text-sm text-ok">
              <span className="font-semibold">{t('study.runner.grading.officialAnswer')}: </span>
              {question.answer.join(', ')}
              {question.grading_mode === 'any_accepted' &&
                question.accepted_answers &&
                question.accepted_answers.length > 0 && (
                  <span className="ml-2 text-xs text-warn">
                    ({question.accepted_answers.map((g) => g.join('+')).join(' / ')})
                  </span>
                )}
            </div>
          )}

          {question.options.length > 0 && (
            <div className="space-y-2">
              {question.options.map((opt) => (
                <div key={opt.key} className="rounded-lg border border-border bg-canvas p-3 text-sm">
                  <div className="font-medium text-ink">
                    <span className="mr-2 font-mono">{opt.key}.</span>
                    {opt.content}
                  </div>
                  {opt.explanation && (
                    <p className="mt-1 text-xs text-ink-muted">{opt.explanation}</p>
                  )}
                  <OptionLinkedCards cardIds={opt.linked_card_ids} onOpenAnchor={onOpenAnchor} />
                </div>
              ))}
            </div>
          )}

          {question.answer_basis && (
            <div className="text-xs text-ink-muted">
              <span className="font-semibold text-ink">{t('study.runner.grading.answerBasis')}: </span>
              {question.answer_basis}
            </div>
          )}

          {question.explanation_blocks && question.explanation_blocks.length > 0 && (
            <div className="space-y-2 border-t border-border pt-3">
              <div className="text-xs font-semibold text-ink-muted">
                {t('study.runner.grading.explanation')}
              </div>
              {question.explanation_blocks.map((block, i) => (
                <BlockRenderer key={i} block={block} />
              ))}
            </div>
          )}

          {entry.errorTags.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {entry.errorTags.map((tag) => (
                <span
                  key={tag}
                  className="rounded-full bg-warn-soft px-2.5 py-0.5 text-xs font-medium text-warn"
                >
                  {t(`study.runner.errorTagLabels.${tag}`)}
                </span>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

export function StudySummary({
  score,
  attemptsWritten,
  entries,
  onOpenAnchor,
}: {
  score: Score
  attemptsWritten: number
  entries: SessionReviewEntry[]
  onOpenAnchor: (anchorId: string) => void
}) {
  const { t } = useTranslation()
  const pct = Math.round(score.ratio * 100)

  return (
    <div className="mx-auto max-w-3xl space-y-6 px-4 py-6 sm:px-6">
      <section className="rounded-2xl border border-border bg-surface p-6">
        <h1 className="text-xl font-bold text-ink">{t('study.runner.summary.title')}</h1>

        <div className="mt-4 flex flex-wrap items-baseline gap-4">
          <div className="font-mono text-4xl font-bold text-accent tabular-nums">{pct}%</div>
          <div className="space-y-1 text-sm text-ink-muted">
            <div>
              {t('study.runner.summary.correctOf', {
                correct: score.correct,
                scoreable: score.scoreable,
              })}
            </div>
            {score.voided > 0 && (
              <div className="text-xs font-medium text-ink-muted">
                {t('study.runner.summary.voidedCount', { count: score.voided })}
              </div>
            )}
            <div className="text-xs">
              {t('study.runner.summary.attemptsSaved', { count: attemptsWritten })}
            </div>
          </div>
        </div>

        <div className="mt-6 flex flex-wrap gap-3">
          <Link
            to="/study"
            className="inline-flex min-h-11 items-center rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover"
          >
            {t('study.runner.summary.backToStudy')}
          </Link>
          <Link
            to="/"
            className="inline-flex min-h-11 items-center rounded-lg border border-border bg-canvas px-4 text-sm font-medium text-ink hover:bg-subtle"
          >
            {t('study.runner.summary.backToReading')}
          </Link>
        </div>
      </section>

      {entries.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-ink-muted">
            {t('study.runner.summary.reviewHeading')}
          </h2>
          {entries.map((entry) => (
            <ReviewEntryItem
              key={`${entry.index}-${entry.question.id}`}
              entry={entry}
              onOpenAnchor={onOpenAnchor}
            />
          ))}
        </section>
      )}
    </div>
  )
}
