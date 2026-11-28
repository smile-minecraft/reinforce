import { GitBranch, Play } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { ApiError } from '../api/client'
import { useGraphNeighbors, useKnowledgeCard } from '../api/hooks/useKnowledgeCards'
import { useCreateSession } from '../api/hooks/useStudySession'
import { AnchorDrawer } from '../components/AnchorDrawer'
import { KnowledgeCardView } from '../components/KnowledgeCardView'
import { saveSessionConfig } from '../study/sessionConfig'
import { useWorkspace } from '../workspace/workspaceContextValue'

export function CardPage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const { openTab, setTabTitle } = useWorkspace()
  const { data: card, isLoading, error } = useKnowledgeCard(id)
  const { data: neighbors = [] } = useGraphNeighbors('card', id, 1)
  const createSession = useCreateSession()

  const [activeAnchorId, setActiveAnchorId] = useState<string | null>(null)
  const [drillError, setDrillError] = useState<string | null>(null)

  useEffect(() => {
    if (id && card?.title) {
      setTabTitle(`card:${id}`, card.title)
    }
  }, [id, card?.title, setTabTitle])

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
      const cfg = { mode: 'topic_drill' as const, secondsPerQuestion: null, totalTimeSecs: null }
      saveSessionConfig(res.session_id, cfg)
      openTab({
        id: `session:${res.session_id}`,
        kind: 'session',
        path: `/study/session/${encodeURIComponent(res.session_id)}`,
        title: t('workspace.tabs.reviewWithTitle', { title: card.title }),
        navState: cfg,
      })
    } catch (err) {
      setDrillError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
        {t('common.loading')}
      </div>
    )
  }

  if (error || !card) {
    return (
      <div className="p-6">
        <div className="rounded-xl bg-bad-soft p-4 text-sm text-bad">
          {error instanceof Error ? error.message : t('common.errorGeneric')}
        </div>
      </div>
    )
  }

  const relatedCards = neighbors.filter((n) => n.endpoint.kind === 'card' && n.endpoint.id !== card.id)

  return (
    <div className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-6 sm:px-8">
      <div className="mx-auto max-w-3xl space-y-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span className="rounded-full border border-border bg-canvas px-3 py-1 text-xs font-medium text-ink-muted">
            {card.subject}
          </span>

          <button
            type="button"
            onClick={() => void drillCard()}
            disabled={createSession.isPending}
            className="inline-flex h-10 items-center gap-2 rounded-full bg-accent px-4 text-xs font-semibold text-white hover:bg-accent-hover disabled:opacity-50"
          >
            <Play className="h-3.5 w-3.5 fill-current" />
            <span>{t('reading.drillThisCard')}</span>
          </button>
        </div>

        {drillError && (
          <div className="rounded-xl border border-warn/40 bg-warn-soft px-4 py-3 text-sm text-warn">
            {drillError}
          </div>
        )}

        <KnowledgeCardView
          card={card}
          onOpenAnchor={setActiveAnchorId}
          onOpenRelatedQuestions={() => void drillCard()}
          openingQuestions={createSession.isPending}
        />

        {/* Related Graph Nodes */}
        <section className="rounded-2xl border border-border bg-canvas/40 p-5">
          <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
            <GitBranch className="h-4 w-4 text-accent" />
            <span>{t('reading.relatedNodes')}</span>
          </h2>

          {relatedCards.length === 0 ? (
            <p className="mt-2 text-sm text-ink-muted">{t('reading.noRelatedNodes')}</p>
          ) : (
            <div className="mt-3 flex flex-wrap gap-2">
              {relatedCards.map((node) => (
                <button
                  key={`${node.endpoint.kind}-${node.endpoint.id}`}
                  type="button"
                  onClick={() =>
                    openTab({
                      id: `card:${node.endpoint.id}`,
                      kind: 'card',
                      path: `/cards/${encodeURIComponent(node.endpoint.id)}`,
                      title: `${t('workspace.tabs.card')} ${node.endpoint.id.slice(-6)}`,
                    })
                  }
                  className="inline-flex h-9 items-center rounded-full border border-border bg-surface px-3 text-xs font-medium text-accent hover:border-accent hover:bg-accent-soft"
                >
                  {t('reading.openCard')} ({node.endpoint.id.slice(-8)})
                </button>
              ))}
            </div>
          )}
        </section>

        <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
      </div>
    </div>
  )
}
