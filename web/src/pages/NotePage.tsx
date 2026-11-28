import {
  Check,
  CheckCircle2,
  Eye,
  EyeOff,
  FileOutput,
  FileText,
  RotateCcw,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { ApiError } from '../api/client'
import { useKnowledgeCardsByIds } from '../api/hooks/useKnowledgeCards'
import { useNote } from '../api/hooks/useNotes'
import { useCreateSession } from '../api/hooks/useStudySession'
import type { KnowledgeCard, OutlineNode } from '../api/types'
import { MarkdownBlock } from '../blocks/MarkdownBlock'
import { AnchorDrawer } from '../components/AnchorDrawer'
import { KnowledgeCardView } from '../components/KnowledgeCardView'
import { saveSessionConfig } from '../study/sessionConfig'
import { RightSidebarSlot } from '../workspace/WorkspaceContext'
import { useWorkspace } from '../workspace/workspaceContextValue'

const CHECKED_STORAGE_PREFIX = 'reinforce:noteChecked:v1:'

function loadCheckedSet(noteId: string | undefined): Set<string> {
  if (!noteId || typeof window === 'undefined') return new Set()
  try {
    const raw = window.localStorage.getItem(`${CHECKED_STORAGE_PREFIX}${noteId}`)
    if (!raw) return new Set()
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? new Set(parsed.filter((x) => typeof x === 'string')) : new Set()
  } catch {
    return new Set()
  }
}

function saveCheckedSet(noteId: string | undefined, checked: Set<string>) {
  if (!noteId || typeof window === 'undefined') return
  try {
    window.localStorage.setItem(
      `${CHECKED_STORAGE_PREFIX}${noteId}`,
      JSON.stringify(Array.from(checked)),
    )
  } catch {
    // Ignore storage quota errors
  }
}

interface OutlineCardEntry {
  key: string
  cardId: string
  sectionId: string
  sectionHeading: string
}

interface ChapterGroup {
  id: string
  heading: string
  introMarkdown: string | null
  depth: number
  cards: OutlineCardEntry[]
}

function flattenOutlineIntoChapters(
  nodes: OutlineNode[],
  fallbackHeading: string,
): { chapters: ChapterGroup[]; allCardIds: string[] } {
  const chapters: ChapterGroup[] = []
  const allCardIds: string[] = []
  let orphanCards: OutlineCardEntry[] = []

  function flushOrphans() {
    if (orphanCards.length > 0) {
      chapters.push({
        id: `sec-root-${chapters.length}`,
        heading: fallbackHeading,
        introMarkdown: null,
        depth: 0,
        cards: orphanCards,
      })
      orphanCards = []
    }
  }

  function walk(list: OutlineNode[], prefix: string, depth: number) {
    list.forEach((node, idx) => {
      if (node.type === 'section') {
        flushOrphans()
        const sectionId = `${prefix}-${idx}`
        const directCards: OutlineCardEntry[] = []
        const nestedSections: OutlineNode[] = []

        node.children.forEach((child, cIdx) => {
          if (child.type === 'card_ref') {
            allCardIds.push(child.card_id)
            directCards.push({
              key: `${sectionId}-card-${cIdx}-${child.card_id}`,
              cardId: child.card_id,
              sectionId,
              sectionHeading: node.heading,
            })
          } else {
            nestedSections.push(child)
          }
        })

        chapters.push({
          id: sectionId,
          heading: node.heading,
          introMarkdown: node.intro_markdown,
          depth,
          cards: directCards,
        })

        if (nestedSections.length > 0) {
          walk(nestedSections, sectionId, depth + 1)
        }
      } else {
        allCardIds.push(node.card_id)
        orphanCards.push({
          key: `${prefix}-card-${idx}-${node.card_id}`,
          cardId: node.card_id,
          sectionId: `${prefix}-root`,
          sectionHeading: fallbackHeading,
        })
      }
    })
  }

  walk(nodes, 'sec', 0)
  flushOrphans()

  return { chapters, allCardIds }
}

export function NotePage() {
  const { id } = useParams()
  const { t } = useTranslation()
  const { openTab, setTabTitle, setMobileRightOpen } = useWorkspace()
  const { data: note, isLoading, error } = useNote(id)
  const createSession = useCreateSession()

  const [activeAnchorId, setActiveAnchorId] = useState<string | null>(null)
  const [checkedByNote, setCheckedByNote] = useState<Record<string, Set<string>>>(() =>
    id ? { [id]: loadCheckedSet(id) } : {},
  )
  const [showChecked, setShowChecked] = useState(false)
  const [reviewError, setReviewError] = useState<string | null>(null)
  const [openingCardId, setOpeningCardId] = useState<string | null>(null)

  const checkedKeys = useMemo(
    () => (id ? (checkedByNote[id] ?? loadCheckedSet(id)) : new Set<string>()),
    [id, checkedByNote],
  )

  useEffect(() => {
    if (id && note?.title) {
      setTabTitle(`note:${id}`, note.title)
    }
  }, [id, note?.title, setTabTitle])

  const { chapters, allCardIds } = useMemo(
    () =>
      note
        ? flattenOutlineIntoChapters(note.outline, note.title)
        : { chapters: [], allCardIds: [] },
    [note],
  )

  const uniqueCardIds = useMemo(() => Array.from(new Set(allCardIds)), [allCardIds])
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

  const allTags = useMemo(() => {
    const tags = new Set<string>()
    if (note) tags.add(note.subject)
    for (const cid of uniqueCardIds) {
      const card = cardMap.get(cid)?.card
      if (card) {
        for (const topic of card.topics) tags.add(topic)
      }
    }
    return Array.from(tags)
  }, [note, uniqueCardIds, cardMap])

  const allEntries = useMemo(() => chapters.flatMap((ch) => ch.cards), [chapters])
  const totalCount = allEntries.length
  const checkedCount = useMemo(
    () => allEntries.filter((entry) => checkedKeys.has(entry.key)).length,
    [allEntries, checkedKeys],
  )
  const allDismissed = totalCount > 0 && checkedCount >= totalCount && !showChecked

  function toggleChecked(entryKey: string) {
    if (!id) return
    setCheckedByNote((prev) => {
      const current = prev[id] ?? loadCheckedSet(id)
      const next = new Set(current)
      if (next.has(entryKey)) next.delete(entryKey)
      else next.add(entryKey)
      saveCheckedSet(id, next)
      return { ...prev, [id]: next }
    })
  }

  function resetChecked() {
    if (!id) return
    const empty = new Set<string>()
    saveCheckedSet(id, empty)
    setCheckedByNote((prev) => ({ ...prev, [id]: empty }))
  }

  function scrollToCard(entryKey: string, cardId: string) {
    if (checkedKeys.has(entryKey) && !showChecked) {
      toggleChecked(entryKey)
    }
    setMobileRightOpen(false)
    window.requestAnimationFrame(() => {
      const el = document.getElementById(`note-entry-${entryKey}`) ?? document.getElementById(`card-${cardId}`)
      el?.scrollIntoView({ behavior: 'smooth', block: 'start' })
    })
  }

  async function openRelatedQuestionsTab(card: KnowledgeCard) {
    setReviewError(null)
    setOpeningCardId(card.id)
    try {
      const res = await createSession.mutateAsync({
        mode: 'topic_drill',
        knowledge_card_ids: [card.id],
      })
      if (res.question_count === 0) {
        setReviewError(`${card.title}：${t('workspace.sidebar.noRelatedQuestions')}`)
        return
      }
      const cfg = {
        mode: 'topic_drill' as const,
        secondsPerQuestion: null,
        totalTimeSecs: null,
      }
      saveSessionConfig(res.session_id, cfg)
      openTab({
        id: `session:${res.session_id}`,
        kind: 'session',
        path: `/study/session/${encodeURIComponent(res.session_id)}`,
        title: t('workspace.tabs.reviewWithTitle', { title: card.title }),
        navState: cfg,
      })
    } catch (err) {
      setReviewError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    } finally {
      setOpeningCardId(null)
    }
  }

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
        {t('common.loading')}
      </div>
    )
  }

  if (error || !note) {
    return (
      <div className="p-6">
        <div className="rounded-xl bg-bad-soft p-4 text-sm text-bad">
          {error instanceof Error ? error.message : t('common.errorGeneric')}
        </div>
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Right Sidebar: Document Outline with Circular Knowledge-Point Checkboxes (Sketch 2) */}
      <RightSidebarSlot>
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <div className="shrink-0 border-b border-border px-3.5 py-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-1.5 text-xs font-semibold text-ink">
                <FileText className="h-3.5 w-3.5 text-accent" />
                <span>{t('workspace.sidebar.documentList')}</span>
              </div>
              <span className="rounded-full bg-subtle px-2 py-0.5 font-mono text-[10px] font-medium text-ink-muted">
                {checkedCount}/{totalCount}
              </span>
            </div>

            {totalCount > 0 && (
              <div className="mt-2.5 flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => setShowChecked((v) => !v)}
                  className={`inline-flex h-7 flex-1 items-center justify-center gap-1 rounded-lg border px-2 text-[11px] font-medium transition-colors ${
                    showChecked
                      ? 'border-accent bg-accent-soft text-accent'
                      : 'border-border bg-surface text-ink-muted hover:text-ink'
                  }`}
                >
                  {showChecked ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
                  <span className="truncate">
                    {showChecked
                      ? t('workspace.sidebar.hideChecked')
                      : t('workspace.sidebar.showChecked')}
                  </span>
                </button>

                {checkedCount > 0 && (
                  <button
                    type="button"
                    onClick={resetChecked}
                    title={t('workspace.sidebar.resetChecked')}
                    className="inline-flex h-7 items-center justify-center gap-1 rounded-lg border border-border bg-surface px-2 text-[11px] font-medium text-ink-muted hover:border-accent hover:text-accent"
                  >
                    <RotateCcw className="h-3 w-3" />
                    <span>{t('workspace.sidebar.resetChecked')}</span>
                  </button>
                )}
              </div>
            )}
          </div>

          <div className="rf-pane-scroll flex-1 space-y-3 overflow-y-auto p-3">
            {chapters.map((chapter) => (
              <div key={chapter.id} className="space-y-1">
                <div
                  style={{ paddingLeft: `${chapter.depth * 10}px` }}
                  className="px-1.5 text-[11px] font-semibold tracking-tight text-ink-muted"
                >
                  {chapter.heading}
                </div>

                <ul className="space-y-0.5">
                  {chapter.cards.map((entry, idx) => {
                    const card = cardMap.get(entry.cardId)?.card
                    const isChecked = checkedKeys.has(entry.key)
                    const label = card?.title ?? `${t('workspace.tabs.card')} ${idx + 1}`

                    return (
                      <li
                        key={entry.key}
                        style={{ paddingLeft: `${chapter.depth * 10 + 4}px` }}
                        className="flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 transition-colors hover:bg-subtle"
                      >
                        <button
                          type="button"
                          onClick={() => scrollToCard(entry.key, entry.cardId)}
                          className={`flex min-w-0 flex-1 items-center gap-2 text-left text-xs transition-opacity ${
                            isChecked
                              ? 'text-ink-muted line-through opacity-55'
                              : 'font-medium text-ink'
                          }`}
                        >
                          <span
                            className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                              isChecked ? 'bg-accent/50' : 'border border-ink-muted/60 bg-transparent'
                            }`}
                          />
                          <span className="truncate">{label}</span>
                        </button>

                        <button
                          type="button"
                          role="checkbox"
                          aria-checked={isChecked}
                          aria-label={label}
                          onClick={() => toggleChecked(entry.key)}
                          className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-full border transition-all duration-200 ${
                            isChecked
                              ? 'scale-105 border-accent bg-accent text-white'
                              : 'border-ink-muted/50 bg-surface text-transparent hover:border-accent'
                          }`}
                        >
                          <Check className="h-3 w-3 stroke-[2.5]" />
                        </button>
                      </li>
                    )
                  })}
                </ul>
              </div>
            ))}
          </div>
        </div>
      </RightSidebarSlot>

      {/* Center Frame Top Title + Tags Header (Sketch 2) */}
      <header className="shrink-0 border-b border-border bg-surface px-5 py-4 sm:px-8">
        <div className="mx-auto flex max-w-3xl flex-wrap items-center justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-2.5">
            <h1 className="text-xl font-bold tracking-tight text-ink sm:text-2xl">{note.title}</h1>
            <div className="flex flex-wrap items-center gap-1.5">
              {allTags.map((tag) => (
                <span
                  key={tag}
                  className="inline-flex items-center rounded-full border border-border bg-canvas px-3 py-0.5 text-xs font-medium text-ink-muted"
                >
                  {tag}
                </span>
              ))}
            </div>
          </div>

          <button
            type="button"
            onClick={() =>
              openTab({
                id: 'export',
                kind: 'export',
                path: '/export',
                title: t('workspace.tabs.export'),
              })
            }
            className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-canvas px-2.5 text-xs font-medium text-ink-muted hover:border-accent hover:text-accent"
          >
            <FileOutput className="h-3.5 w-3.5" />
            <span>{t('nav.exportShort')}</span>
          </button>
        </div>
      </header>

      {/* Center Frame Scrollable Linear Note Content */}
      <div className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-6 sm:px-8">
        <div className="mx-auto max-w-3xl space-y-6">
          {reviewError && (
            <div className="rounded-xl border border-warn/40 bg-warn-soft px-4 py-3 text-xs font-medium text-warn">
              {reviewError}
            </div>
          )}

          {allDismissed && (
            <section className="rounded-2xl border border-accent/30 bg-accent-soft/50 p-8 text-center">
              <CheckCircle2 className="mx-auto h-9 w-9 text-accent" />
              <h2 className="mt-3 text-base font-semibold text-ink">
                {t('workspace.sidebar.allCheckedTitle')}
              </h2>
              <p className="mt-1 text-xs text-ink-muted">
                {t('workspace.sidebar.allCheckedDesc')}
              </p>
              <div className="mt-4 flex flex-wrap items-center justify-center gap-2">
                <button
                  type="button"
                  onClick={resetChecked}
                  className="inline-flex h-9 items-center gap-1.5 rounded-lg bg-accent px-4 text-xs font-medium text-white hover:bg-accent-hover"
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  <span>{t('workspace.sidebar.resetChecked')}</span>
                </button>
                <button
                  type="button"
                  onClick={() => setShowChecked(true)}
                  className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-border bg-surface px-4 text-xs font-medium text-ink hover:bg-subtle"
                >
                  <Eye className="h-3.5 w-3.5" />
                  <span>{t('workspace.sidebar.showChecked')}</span>
                </button>
              </div>
            </section>
          )}

          {chapters.map((chapter) => {
            const visibleCards = chapter.cards.filter(
              (entry) => showChecked || !checkedKeys.has(entry.key),
            )
            const chapterHidden =
              chapter.cards.length > 0 && visibleCards.length === 0 && !showChecked

            return (
              <section
                key={chapter.id}
                id={chapter.id}
                className="rf-collapse-item"
                data-collapsed={chapterHidden}
              >
                <div className="rf-collapse-inner">
                  <div className="mb-2 border-b border-border pb-2">
                    {chapter.depth === 0 ? (
                      <h2 className="text-base font-semibold tracking-tight text-accent">
                        {chapter.heading}
                      </h2>
                    ) : (
                      <h3 className="text-sm font-semibold tracking-tight text-ink">
                        {chapter.heading}
                      </h3>
                    )}
                    {chapter.introMarkdown && (
                      <div className="mt-1.5">
                        <MarkdownBlock text={chapter.introMarkdown} className="text-sm text-ink-muted" />
                      </div>
                    )}
                  </div>

                  <div>
                    {chapter.cards.map((entry) => {
                      const isChecked = checkedKeys.has(entry.key)
                      const isCollapsed = isChecked && !showChecked
                      const queryEntry = cardMap.get(entry.cardId)

                      return (
                        <div
                          key={entry.key}
                          id={`note-entry-${entry.key}`}
                          className="rf-collapse-item scroll-mt-4"
                          data-collapsed={isCollapsed}
                        >
                          <div className="rf-collapse-inner">
                            {!queryEntry || queryEntry.loading ? (
                              <div className="border-b border-border py-5 text-xs text-ink-muted">
                                {t('common.loading')}
                              </div>
                            ) : queryEntry.error || !queryEntry.card ? (
                              <div className="my-3 rounded-xl border border-bad/40 bg-bad-soft p-4 text-xs text-bad">
                                {t('reading.missingCard')}
                              </div>
                            ) : (
                              <KnowledgeCardView
                                card={queryEntry.card}
                                variant="linear"
                                checked={isChecked}
                                onToggleChecked={() => toggleChecked(entry.key)}
                                onOpenAnchor={setActiveAnchorId}
                                onOpenRelatedQuestions={(c) => void openRelatedQuestionsTab(c)}
                                openingQuestions={openingCardId === queryEntry.card.id}
                                showCardLink
                              />
                            )}
                          </div>
                        </div>
                      )
                    })}
                  </div>
                </div>
              </section>
            )
          })}
        </div>
      </div>

      <AnchorDrawer anchorId={activeAnchorId} onClose={() => setActiveAnchorId(null)} />
    </div>
  )
}
