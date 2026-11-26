import { ArrowLeft, ListTree } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useParams } from 'react-router'
import { useKnowledgeCardsByIds } from '../api/hooks/useKnowledgeCards'
import { useNote } from '../api/hooks/useNotes'
import type { KnowledgeCard, OutlineNode } from '../api/types'
import { MarkdownBlock } from '../blocks/MarkdownBlock'
import { AnchorDrawer } from '../components/AnchorDrawer'
import { KnowledgeCardView } from '../components/KnowledgeCardView'

function collectCardIds(nodes: OutlineNode[]): string[] {
  const ids: string[] = []
  for (const node of nodes) {
    if (node.type === 'card_ref') ids.push(node.card_id)
    else ids.push(...collectCardIds(node.children))
  }
  return ids
}

interface TocItem {
  id: string
  heading: string
  depth: number
}

function collectToc(nodes: OutlineNode[], prefix = 'sec', depth = 0): TocItem[] {
  const items: TocItem[] = []
  nodes.forEach((node, idx) => {
    if (node.type === 'section') {
      const id = `${prefix}-${idx}`
      items.push({ id, heading: node.heading, depth })
      items.push(...collectToc(node.children, id, depth + 1))
    }
  })
  return items
}

function OutlineBranch({
  nodes,
  cardMap,
  onOpenAnchor,
  prefix = 'sec',
  depth = 0,
}: {
  nodes: OutlineNode[]
  cardMap: Map<string, { card?: KnowledgeCard; loading: boolean; error: boolean }>
  onOpenAnchor: (anchorId: string) => void
  prefix?: string
  depth?: number
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-5">
      {nodes.map((node, idx) => {
        if (node.type === 'section') {
          const sectionId = `${prefix}-${idx}`
          return (
            <section key={sectionId} id={sectionId} className="scroll-mt-20 space-y-4">
              <div className="border-b border-border pb-2">
                {depth === 0 ? (
                  <h2 className="text-xl font-semibold tracking-tight text-ink">{node.heading}</h2>
                ) : (
                  <h3 className="text-lg font-semibold tracking-tight text-ink">{node.heading}</h3>
                )}
                {node.intro_markdown && (
                  <div className="mt-2">
                    <MarkdownBlock text={node.intro_markdown} className="text-ink-muted" />
                  </div>
                )}
              </div>

              <div className={depth > 0 ? 'pl-3 sm:pl-5 border-l border-border' : ''}>
                <OutlineBranch
                  nodes={node.children}
                  cardMap={cardMap}
                  onOpenAnchor={onOpenAnchor}
                  prefix={sectionId}
                  depth={depth + 1}
                />
              </div>
            </section>
          )
        }

        const entry = cardMap.get(node.card_id)
        if (!entry || entry.loading) {
          return (
            <div
              key={`${prefix}-card-${idx}-${node.card_id}`}
              className="rounded-xl border border-border bg-surface p-5 text-sm text-ink-muted"
            >
              {t('common.loading')}
            </div>
          )
        }
        if (entry.error || !entry.card) {
          return (
            <div
              key={`${prefix}-card-${idx}-${node.card_id}`}
              className="rounded-xl border border-bad/40 bg-bad-soft p-4 text-sm text-bad"
            >
              {t('reading.missingCard')}
            </div>
          )
        }

        return (
          <KnowledgeCardView
            key={`${prefix}-card-${idx}-${node.card_id}`}
            card={entry.card}
            onOpenAnchor={onOpenAnchor}
            showCardLink
          />
        )
      })}
    </div>
  )
}

export function NotePage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const { data: note, isLoading, error } = useNote(id)
  const [activeAnchorId, setActiveAnchorId] = useState<string | null>(null)

  const cardIds = useMemo(() => (note ? collectCardIds(note.outline) : []), [note])
  const uniqueCardIds = useMemo(() => Array.from(new Set(cardIds)), [cardIds])
  const cardQueries = useKnowledgeCardsByIds(uniqueCardIds)

  const cardMap = useMemo(() => {
    const map = new Map<string, { card?: KnowledgeCard; loading: boolean; error: boolean }>()
    uniqueCardIds.forEach((cid, i) => {
      const q = cardQueries[i]
      map.set(cid, {
        card: q?.data,
        loading: q?.isLoading ?? false,
        error: q?.isError ?? false,
      })
    })
    return map
  }, [uniqueCardIds, cardQueries])

  const toc = useMemo(() => (note ? collectToc(note.outline) : []), [note])

  if (isLoading) {
    return <div className="mx-auto max-w-4xl px-4 py-12 text-center text-sm text-ink-muted">{t('common.loading')}</div>
  }

  if (error || !note) {
    return (
      <div className="mx-auto max-w-4xl px-4 py-8">
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

  return (
    <div className="mx-auto max-w-6xl px-4 py-5 sm:px-6 lg:py-8">
      <Link
        to="/"
        className="inline-flex min-h-11 items-center gap-1.5 text-sm font-medium text-ink-muted hover:text-ink"
      >
        <ArrowLeft className="h-4 w-4" />
        <span>{t('nav.reading')}</span>
      </Link>

      <div className="mt-3 lg:grid lg:grid-cols-[minmax(0,1fr)_220px] lg:gap-8">
        {/* Main Reading Column */}
        <div className="min-w-0 max-w-3xl space-y-6">
          <header className="space-y-2">
            <span className="inline-block rounded bg-subtle px-2.5 py-0.5 text-xs font-medium text-ink-muted">
              {note.subject}
            </span>
            <h1 className="text-2xl font-bold tracking-tight text-ink sm:text-3xl">{note.title}</h1>
          </header>

          <OutlineBranch
            nodes={note.outline}
            cardMap={cardMap}
            onOpenAnchor={setActiveAnchorId}
          />
        </div>

        {/* Sticky Outline TOC on Desktop / iPad Landscape */}
        {toc.length > 0 && (
          <aside className="hidden lg:block">
            <div className="sticky top-8 rounded-xl border border-border bg-surface p-4">
              <div className="mb-2.5 flex items-center gap-1.5 text-xs font-semibold text-ink-muted">
                <ListTree className="h-3.5 w-3.5" />
                <span>{t('reading.outline')}</span>
              </div>
              <ul className="space-y-1">
                {toc.map((item) => (
                  <li key={item.id} style={{ paddingLeft: `${item.depth * 12}px` }}>
                    <a
                      href={`#${item.id}`}
                      className="block rounded-md px-2 py-1.5 text-xs text-ink-muted hover:bg-subtle hover:text-ink"
                    >
                      {item.heading}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          </aside>
        )}
      </div>

      <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
    </div>
  )
}
