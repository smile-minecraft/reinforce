import { CheckCircle2, Clock, FileSearch, ArrowUpRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import type { KnowledgeCard } from '../api/types'
import { BlockRenderer } from '../blocks/BlockRenderer'

export function KnowledgeCardView({
  card,
  onOpenAnchor,
  compact = false,
  showCardLink = false,
}: {
  card: KnowledgeCard
  onOpenAnchor?: (anchorId: string) => void
  compact?: boolean
  showCardLink?: boolean
}) {
  const { t } = useTranslation()

  return (
    <article
      id={`card-${card.id}`}
      className={`rounded-xl border border-border bg-surface ${compact ? 'p-4' : 'p-5 sm:p-6'}`}
    >
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className={`${compact ? 'text-base' : 'text-lg'} font-semibold tracking-tight text-ink`}>
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
              <span className="rounded bg-subtle px-2 py-0.5 text-xs font-medium text-ink-muted">
                {card.subject}
              </span>
              {card.topics.map((topic) => (
                <span
                  key={topic}
                  className="rounded border border-border px-2 py-0.5 text-xs text-ink-muted"
                >
                  {topic}
                </span>
              ))}
            </div>
          )}
        </div>

        {showCardLink && (
          <Link
            to={`/cards/${encodeURIComponent(card.id)}`}
            className="inline-flex min-h-11 shrink-0 items-center gap-1 rounded-lg px-2.5 text-xs font-medium text-accent hover:bg-accent-soft"
          >
            <span>{t('reading.openCard')}</span>
            <ArrowUpRight className="h-3.5 w-3.5" />
          </Link>
        )}
      </header>

      <div className="mt-4 space-y-3">
        {card.blocks.map((block, idx) => (
          <BlockRenderer key={`${card.id}-block-${idx}`} block={block} />
        ))}
      </div>

      {card.anchor_ids.length > 0 && onOpenAnchor && (
        <footer className="mt-4 flex flex-wrap items-center gap-2 border-t border-border pt-3">
          <span className="text-xs font-medium text-ink-muted">{t('reading.sources')}:</span>
          {card.anchor_ids.map((anchorId, idx) => (
            <button
              key={anchorId}
              type="button"
              onClick={() => onOpenAnchor(anchorId)}
              className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:border-accent hover:text-accent"
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
