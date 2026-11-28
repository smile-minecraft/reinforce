import {
  ArrowUpRight,
  Check,
  CheckCircle2,
  Clock,
  FileSearch,
  GraduationCap,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useCardQuestions } from '../api/hooks/useKnowledgeCards'
import type { KnowledgeCard } from '../api/types'
import { BlockRenderer } from '../blocks/BlockRenderer'
import { useWorkspace } from '../workspace/workspaceContextValue'

export function KnowledgeCardView({
  card,
  onOpenAnchor,
  onOpenRelatedQuestions,
  openingQuestions = false,
  checked = false,
  onToggleChecked,
  compact = false,
  showCardLink = false,
  variant = 'card',
}: {
  card: KnowledgeCard
  onOpenAnchor?: (anchorId: string) => void
  onOpenRelatedQuestions?: (card: KnowledgeCard) => void
  openingQuestions?: boolean
  checked?: boolean
  onToggleChecked?: (cardId: string) => void
  compact?: boolean
  showCardLink?: boolean
  variant?: 'card' | 'linear'
}) {
  const { t } = useTranslation()
  const { openTab } = useWorkspace()
  const { data: related } = useCardQuestions(onOpenRelatedQuestions ? card.id : undefined)
  const relatedCount = related?.count ?? 0

  const containerClass =
    variant === 'linear'
      ? 'border-b border-border bg-surface py-6'
      : `rounded-xl border border-border bg-surface ${compact ? 'p-4' : 'p-5 sm:p-6'}`

  return (
    <article id={`card-${card.id}`} className={containerClass}>
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            {onToggleChecked && (
              <button
                type="button"
                onClick={() => onToggleChecked(card.id)}
                aria-pressed={checked}
                title={checked ? t('workspace.sidebar.hideChecked') : t('common.confirm')}
                className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full border transition-colors ${
                  checked
                    ? 'border-accent bg-accent text-white'
                    : 'border-border bg-surface text-transparent hover:border-accent'
                }`}
              >
                <Check className="h-3 w-3" />
              </button>
            )}
            <h3
              className={`${compact ? 'text-base' : 'text-lg'} font-semibold tracking-tight text-ink ${
                checked ? 'line-through opacity-60' : ''
              }`}
            >
              {card.title}
            </h3>
            <span
              className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium ${
                card.verification === 'verified'
                  ? 'bg-ok-soft text-ok'
                  : 'bg-warn-soft text-warn'
              }`}
            >
              {card.verification === 'verified' ? (
                <CheckCircle2 className="h-3 w-3" />
              ) : (
                <Clock className="h-3 w-3" />
              )}
              {card.verification === 'verified' ? t('common.verified') : t('common.unchecked')}
            </span>
          </div>

          {card.topics.length > 0 && (
            <div className="mt-1.5 flex flex-wrap gap-1.5">
              {card.topics.map((topic) => (
                <span
                  key={topic}
                  className="rounded-full border border-border bg-canvas px-2.5 py-0.5 text-[11px] text-ink-muted"
                >
                  # {topic}
                </span>
              ))}
            </div>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-1.5">
          {onOpenRelatedQuestions && (
            <button
              type="button"
              onClick={() => onOpenRelatedQuestions(card)}
              disabled={openingQuestions}
              className="inline-flex h-9 items-center gap-1.5 rounded-full border border-accent/40 bg-accent-soft px-3 text-xs font-medium text-accent transition-colors hover:bg-accent hover:text-white disabled:opacity-50"
            >
              <GraduationCap className="h-3.5 w-3.5" />
              <span>
                {relatedCount > 0
                  ? t('workspace.sidebar.relatedQuestionsCount', { count: relatedCount })
                  : t('workspace.sidebar.relatedQuestions')}
              </span>
            </button>
          )}

          {showCardLink && (
            <button
              type="button"
              onClick={() =>
                openTab({
                  id: `card:${card.id}`,
                  kind: 'card',
                  path: `/cards/${encodeURIComponent(card.id)}`,
                  title: card.title,
                })
              }
              className="inline-flex h-9 shrink-0 items-center gap-1 rounded-lg px-2 text-xs font-medium text-ink-muted hover:bg-subtle hover:text-ink"
            >
              <span>{t('reading.openCard')}</span>
              <ArrowUpRight className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </header>

      <div className="mt-4 space-y-3">
        {card.blocks.map((block, idx) => (
          <BlockRenderer key={`${card.id}-block-${idx}`} block={block} />
        ))}
      </div>

      {card.anchor_ids.length > 0 && onOpenAnchor && (
        <footer className="mt-4 flex flex-wrap items-center gap-2 pt-2">
          <span className="text-xs font-medium text-ink-muted">{t('reading.sources')}:</span>
          {card.anchor_ids.map((anchorId, idx) => (
            <button
              key={anchorId}
              type="button"
              onClick={() => onOpenAnchor(anchorId)}
              className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-canvas px-2.5 text-xs font-medium text-ink hover:border-accent hover:text-accent"
            >
              <FileSearch className="h-3.5 w-3.5 text-accent" />
              <span>
                {t('reading.sourceAnchor')} #{idx + 1}
              </span>
            </button>
          ))}
        </footer>
      )}
    </article>
  )
}
