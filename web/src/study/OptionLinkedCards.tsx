import { ChevronDown, ChevronUp, Layers } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useKnowledgeCardsByIds } from '../api/hooks/useKnowledgeCards'
import { KnowledgeCardView } from '../components/KnowledgeCardView'

function LoadedCardsList({
  cardIds,
  onOpenAnchor,
}: {
  cardIds: string[]
  onOpenAnchor?: (anchorId: string) => void
}) {
  const { t } = useTranslation()
  const queries = useKnowledgeCardsByIds(cardIds)

  return (
    <div className="mt-2.5 space-y-2.5">
      {queries.map((q, idx) => {
        if (q.isLoading) {
          return (
            <div key={cardIds[idx]} className="rounded-lg bg-subtle p-3 text-xs text-ink-muted">
              {t('common.loading')}
            </div>
          )
        }
        if (!q.data) return null
        return (
          <KnowledgeCardView
            key={q.data.id}
            card={q.data}
            compact
            showCardLink
            onOpenAnchor={onOpenAnchor}
          />
        )
      })}
    </div>
  )
}

export function OptionLinkedCards({
  cardIds,
  onOpenAnchor,
}: {
  cardIds: string[]
  onOpenAnchor?: (anchorId: string) => void
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)

  if (cardIds.length === 0) return null

  return (
    <div className="mt-2">
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation()
          setExpanded((v) => !v)
        }}
        aria-expanded={expanded}
        className="inline-flex min-h-10 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-accent hover:bg-accent-soft"
      >
        <Layers className="h-3.5 w-3.5" />
        <span>
          {expanded
            ? t('study.runner.grading.hideLinkedCards')
            : t('study.runner.grading.linkedCards', { count: cardIds.length })}
        </span>
        {expanded ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
      </button>

      {expanded && <LoadedCardsList cardIds={cardIds} onOpenAnchor={onOpenAnchor} />}
    </div>
  )
}
