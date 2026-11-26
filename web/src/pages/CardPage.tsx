import { ArrowLeft, GitBranch, Play } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useParams } from 'react-router'
import { ApiError } from '../api/client'
import { useGraphNeighbors, useKnowledgeCard } from '../api/hooks/useKnowledgeCards'
import { useCreateSession } from '../api/hooks/useStudySession'
import { AnchorDrawer } from '../components/AnchorDrawer'
import { KnowledgeCardView } from '../components/KnowledgeCardView'

export function CardPage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { data: card, isLoading, error } = useKnowledgeCard(id)
  const { data: neighbors = [] } = useGraphNeighbors('card', id, 1)
  const createSession = useCreateSession()

  const [activeAnchorId, setActiveAnchorId] = useState<string | null>(null)
  const [drillError, setDrillError] = useState<string | null>(null)

  async function drillCard() {
    if (!card) return
    setDrillError(null)
    try {
      const res = await createSession.mutateAsync({
        mode: 'topic_drill',
        knowledge_card_ids: [card.id],
      })
      if (res.question_count === 0) {
        setDrillError(t('study.options.emptySessionError'))
        return
      }
      void navigate(`/study/session/${encodeURIComponent(res.session_id)}`, {
        state: { mode: 'topic_drill' },
      })
    } catch (err) {
      setDrillError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  if (isLoading) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-12 text-center text-sm text-ink-muted">
        {t('common.loading')}
      </div>
    )
  }

  if (error || !card) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-8">
        <Link to="/" className="inline-flex min-h-11 items-center gap-1.5 text-sm text-accent">
          <ArrowLeft className="h-4 w-4" />
          <span>{t('common.back')}</span>
        </Link>
        <div className="mt-4 rounded-xl bg-bad-soft p-4 text-sm text-bad">
          {error instanceof Error ? error.message : t('common.errorGeneric')}
        </div>
      </div>
    )
  }

  const relatedCards = neighbors.filter((n) => n.endpoint.kind === 'card' && n.endpoint.id !== card.id)

  return (
    <div className="mx-auto max-w-3xl space-y-6 px-4 py-5 sm:px-6 lg:py-8">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Link
          to="/"
          className="inline-flex min-h-11 items-center gap-1.5 text-sm font-medium text-ink-muted hover:text-ink"
        >
          <ArrowLeft className="h-4 w-4" />
          <span>{t('nav.reading')}</span>
        </Link>

        <button
          type="button"
          onClick={() => void drillCard()}
          disabled={createSession.isPending}
          className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          <Play className="h-4 w-4 fill-current" />
          <span>{t('reading.drillThisCard')}</span>
        </button>
      </div>

      {drillError && (
        <div className="rounded-lg border border-warn/40 bg-warn-soft px-4 py-3 text-sm text-warn">
          {drillError}
        </div>
      )}

      <KnowledgeCardView card={card} onOpenAnchor={setActiveAnchorId} />

      {/* Related Graph Nodes */}
      <section className="rounded-xl border border-border bg-surface p-5">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
          <GitBranch className="h-4 w-4 text-accent" />
          <span>{t('reading.relatedNodes')}</span>
        </h2>

        {relatedCards.length === 0 ? (
          <p className="mt-2 text-sm text-ink-muted">{t('reading.noRelatedNodes')}</p>
        ) : (
          <div className="mt-3 flex flex-wrap gap-2">
            {relatedCards.map((node) => (
              <Link
                key={`${node.endpoint.kind}-${node.endpoint.id}`}
                to={`/cards/${encodeURIComponent(node.endpoint.id)}`}
                className="inline-flex min-h-11 items-center rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-accent hover:border-accent hover:bg-accent-soft"
              >
                {t('reading.openCard')} ({node.endpoint.id.slice(-8)})
              </Link>
            ))}
          </div>
        )}
      </section>

      <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
    </div>
  )
}
