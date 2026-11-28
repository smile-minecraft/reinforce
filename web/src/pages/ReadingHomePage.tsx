import { BookOpen, Flame, FolderOpen, Layers, Play, Search, Sparkles, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '../api/client'
import { useKnowledgeCards } from '../api/hooks/useKnowledgeCards'
import { useNotes } from '../api/hooks/useNotes'
import { useSearch } from '../api/hooks/useQuestions'
import { useCreateSession } from '../api/hooks/useStudySession'
import { useSubjectGroups } from '../api/hooks/useSubjectGroups'
import type { OutlineNode, SessionMode, SubjectGroup } from '../api/types'
import { SubjectGroupTree } from '../components/SubjectGroupTree'
import { saveSessionConfig } from '../study/sessionConfig'
import { RightSidebarSlot } from '../workspace/WorkspaceContext'
import { useWorkspace } from '../workspace/workspaceContextValue'
import { resolveGroupSubjects } from '../workspace/workspaceLogic'

function countCardsInOutline(nodes: OutlineNode[]): number {
  let count = 0
  for (const node of nodes) {
    if (node.type === 'card_ref') count += 1
    else count += countCardsInOutline(node.children)
  }
  return count
}

export function ReadingHomePage() {
  const { t } = useTranslation()
  const { openTab } = useWorkspace()
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null)
  const [selectedSubject, setSelectedSubject] = useState<string>('')
  const [searchInput, setSearchInput] = useState('')
  const [launchError, setLaunchError] = useState<string | null>(null)

  const { data: notes = [], isLoading: loadingNotes, error: notesError } = useNotes()
  const { data: cards = [], isLoading: loadingCards } = useKnowledgeCards()
  const { data: subjectGroups = [] } = useSubjectGroups()
  const { data: searchResults = [], isFetching: searching } = useSearch(searchInput)
  const createSession = useCreateSession()

  const subjects = useMemo(() => {
    const set = new Set<string>()
    for (const n of notes) set.add(n.subject)
    for (const c of cards) set.add(c.subject)
    return Array.from(set).sort()
  }, [notes, cards])

  const activeGroup = useMemo(
    () => (selectedGroupId ? subjectGroups.find((g) => g.id === selectedGroupId) ?? null : null),
    [subjectGroups, selectedGroupId],
  )

  const groupSubjects = useMemo(
    () => (selectedGroupId ? resolveGroupSubjects(subjectGroups, selectedGroupId) : null),
    [subjectGroups, selectedGroupId],
  )

  const filteredNotes = useMemo(() => {
    let list = notes
    if (groupSubjects) {
      const allowed = new Set(groupSubjects)
      list = list.filter((n) => allowed.has(n.subject))
    }
    if (selectedSubject) {
      list = list.filter((n) => n.subject === selectedSubject)
    }
    return list
  }, [notes, groupSubjects, selectedSubject])

  const filteredCards = useMemo(() => {
    let list = cards
    if (groupSubjects) {
      const allowed = new Set(groupSubjects)
      list = list.filter((c) => allowed.has(c.subject))
    }
    if (selectedSubject) {
      list = list.filter((c) => c.subject === selectedSubject)
    }
    return list
  }, [cards, groupSubjects, selectedSubject])

  async function quickStart(mode: SessionMode, groupOverride?: SubjectGroup) {
    setLaunchError(null)
    const targetGroup = groupOverride ?? activeGroup
    try {
      const res = await createSession.mutateAsync({
        mode,
        subject_group_id: targetGroup?.id,
        subject: !targetGroup && selectedSubject ? selectedSubject : undefined,
      })
      if (res.question_count === 0) {
        setLaunchError(t('study.options.emptySessionError'))
        return
      }
      const cfg = { mode, secondsPerQuestion: null, totalTimeSecs: null }
      saveSessionConfig(res.session_id, cfg)
      const label = targetGroup
        ? t('workspace.tabs.reviewWithTitle', { title: targetGroup.name })
        : t('workspace.tabs.reviewWithTitle', { title: t(`study.modes.${mode}.name`) })
      openTab({
        id: `session:${res.session_id}`,
        kind: 'session',
        path: `/study/session/${encodeURIComponent(res.session_id)}`,
        title: label,
        navState: cfg,
      })
    } catch (err) {
      setLaunchError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  const hasSearch = searchInput.trim().length > 0

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Right Sidebar: Nested Subject Groups Tree */}
      <RightSidebarSlot>
        <div className="rf-pane-scroll flex-1 overflow-y-auto p-3.5">
          <SubjectGroupTree
            groups={subjectGroups}
            availableSubjects={subjects}
            selectedGroupId={selectedGroupId}
            onSelectGroup={(gid) => {
              setSelectedGroupId(gid)
              setSelectedSubject('')
            }}
            onReviewGroup={(group) => void quickStart('daily_due', group)}
            editable
          />
        </div>
      </RightSidebarSlot>

      <div className="rf-pane-scroll flex-1 overflow-y-auto px-5 py-5 sm:px-8">
        <div className="mx-auto max-w-4xl space-y-6">
          {/* Quick Study Strip */}
          <section className="flex flex-col justify-between gap-4 rounded-2xl border border-border bg-canvas/70 p-4 sm:flex-row sm:items-center sm:p-5">
            <div className="min-w-0">
              <div className="flex items-center gap-2 text-base font-semibold text-ink">
                <Sparkles className="h-4 w-4 text-accent" />
                <h1>
                  {activeGroup ? activeGroup.name : t('reading.dailyDueBannerTitle')}
                </h1>
              </div>
              <p className="mt-0.5 text-xs text-ink-muted">
                {activeGroup
                  ? groupSubjects && groupSubjects.length > 0
                    ? t('workspace.groups.selectedGroupSummary', {
                        count: groupSubjects.length,
                        subjects: groupSubjects.join(', '),
                      })
                    : t('workspace.groups.groupHasNoSubjects')
                  : t('reading.dailyDueBannerDesc')}
              </p>
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                disabled={createSession.isPending}
                onClick={() => void quickStart('daily_due')}
                className="inline-flex h-10 items-center gap-2 rounded-xl bg-accent px-4 text-xs font-semibold text-white hover:bg-accent-hover disabled:opacity-50"
              >
                <Play className="h-3.5 w-3.5 fill-current" />
                <span>
                  {activeGroup
                    ? t('workspace.groups.reviewThisGroup')
                    : t('reading.startDailyDue')}
                </span>
              </button>

              <button
                type="button"
                disabled={createSession.isPending}
                onClick={() => void quickStart('weakness_drill')}
                className="inline-flex h-10 items-center gap-2 rounded-xl border border-border bg-surface px-4 text-xs font-medium text-ink hover:bg-subtle disabled:opacity-50"
              >
                <Flame className="h-3.5 w-3.5 text-warn" />
                <span>{t('reading.startWeakness')}</span>
              </button>
            </div>
          </section>

          {launchError && (
            <div className="rounded-xl border border-warn/40 bg-warn-soft px-4 py-3 text-xs font-medium text-warn">
              {launchError}
            </div>
          )}

          {/* Search & Subject Chips */}
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
            <div className="relative flex-1">
              <Search className="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-ink-muted" />
              <input
                type="search"
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                placeholder={t('common.search')}
                className="h-10 w-full rounded-xl border border-border bg-canvas pr-9 pl-10 text-sm text-ink placeholder:text-ink-muted"
              />
              {hasSearch && (
                <button
                  type="button"
                  onClick={() => setSearchInput('')}
                  aria-label={t('common.close')}
                  className="absolute top-1/2 right-1 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-md text-ink-muted hover:bg-subtle"
                >
                  <X className="h-4 w-4" />
                </button>
              )}
            </div>

            {subjects.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                {activeGroup && (
                  <button
                    type="button"
                    onClick={() => setSelectedGroupId(null)}
                    className="inline-flex h-9 items-center gap-1 rounded-full border border-accent bg-accent-soft px-3 text-xs font-medium text-accent"
                  >
                    <FolderOpen className="h-3.5 w-3.5" />
                    <span>{activeGroup.name}</span>
                    <X className="h-3 w-3" />
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    setSelectedSubject('')
                    setSelectedGroupId(null)
                  }}
                  className={`h-9 rounded-full border px-3 text-xs font-medium transition-colors ${
                    selectedSubject === '' && selectedGroupId === null
                      ? 'border-accent bg-accent-soft text-accent'
                      : 'border-border bg-surface text-ink-muted hover:text-ink'
                  }`}
                >
                  {t('common.allSubjects')}
                </button>
                {subjects.map((subj) => (
                  <button
                    key={subj}
                    type="button"
                    onClick={() => setSelectedSubject(selectedSubject === subj ? '' : subj)}
                    className={`h-9 rounded-full border px-3 text-xs font-medium transition-colors ${
                      selectedSubject === subj
                        ? 'border-accent bg-accent-soft text-accent'
                        : 'border-border bg-surface text-ink-muted hover:text-ink'
                    }`}
                  >
                    {subj}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Search Results or Notes & Knowledge Cards */}
          {hasSearch ? (
            <section className="space-y-3">
              <h2 className="text-xs font-semibold text-ink-muted">
                {t('reading.searchResultsHeading')}
              </h2>

              {searching ? (
                <div className="py-8 text-center text-sm text-ink-muted">{t('common.loading')}</div>
              ) : searchResults.length === 0 ? (
                <div className="rounded-xl border border-border bg-canvas/50 p-8 text-center text-sm text-ink-muted">
                  {t('reading.noSearchResults')}
                </div>
              ) : (
                <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
                  {searchResults.map((hit) => {
                    if (hit.kind === 'note') {
                      return (
                        <li key={`note-${hit.id}`}>
                          <button
                            type="button"
                            onClick={() =>
                              openTab({
                                id: `note:${hit.id}`,
                                kind: 'note',
                                path: `/notes/${encodeURIComponent(hit.id)}`,
                                title: hit.title,
                              })
                            }
                            className="flex min-h-12 w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-subtle/60"
                          >
                            <span className="font-medium text-ink">{hit.title}</span>
                            <span className="rounded-full bg-subtle px-2.5 py-0.5 text-xs text-ink-muted">
                              {t('reading.notesHeading')}
                            </span>
                          </button>
                        </li>
                      )
                    }
                    if (hit.kind === 'knowledge_card') {
                      return (
                        <li key={`card-${hit.id}`}>
                          <button
                            type="button"
                            onClick={() =>
                              openTab({
                                id: `card:${hit.id}`,
                                kind: 'card',
                                path: `/cards/${encodeURIComponent(hit.id)}`,
                                title: hit.title,
                              })
                            }
                            className="flex min-h-12 w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-subtle/60"
                          >
                            <span className="font-medium text-ink">{hit.title}</span>
                            <span className="rounded-full bg-accent-soft px-2.5 py-0.5 text-xs text-accent">
                              {t('reading.cardsHeading')}
                            </span>
                          </button>
                        </li>
                      )
                    }
                    const previewText = hit.title
                      .map((b) => (b.type === 'markdown' ? b.text : b.type === 'math' ? b.latex : ''))
                      .join(' ')
                      .slice(0, 120)
                    return (
                      <li
                        key={`q-${hit.id}`}
                        className="flex min-h-12 items-center justify-between gap-3 px-4 py-3"
                      >
                        <span className="truncate text-sm text-ink">{previewText || hit.id}</span>
                        <span className="shrink-0 rounded-full bg-subtle px-2 py-0.5 text-xs text-ink-muted">
                          Q
                        </span>
                      </li>
                    )
                  })}
                </ul>
              )}
            </section>
          ) : (
            <>
              {notesError && (
                <div className="rounded-xl bg-bad-soft p-4 text-sm text-bad">
                  {notesError instanceof Error ? notesError.message : t('common.errorGeneric')}
                </div>
              )}

              {/* Notes List */}
              <section className="space-y-3">
                <div className="flex items-center justify-between">
                  <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
                    <BookOpen className="h-4 w-4 text-accent" />
                    <span>{t('reading.notesHeading')}</span>
                  </h2>
                  <span className="text-xs text-ink-muted">
                    {t('reading.subjectCount', { count: filteredNotes.length })}
                  </span>
                </div>

                {loadingNotes ? (
                  <div className="py-8 text-center text-sm text-ink-muted">{t('common.loading')}</div>
                ) : filteredNotes.length === 0 ? (
                  <div className="rounded-xl border border-border bg-canvas/50 p-8 text-center text-sm text-ink-muted">
                    {t('reading.noNotes')}
                  </div>
                ) : (
                  <div className="grid gap-3 sm:grid-cols-2">
                    {filteredNotes.map((note) => {
                      const cardCount = countCardsInOutline(note.outline)
                      return (
                        <button
                          key={note.id}
                          type="button"
                          onClick={() =>
                            openTab({
                              id: `note:${note.id}`,
                              kind: 'note',
                              path: `/notes/${encodeURIComponent(note.id)}`,
                              title: note.title,
                            })
                          }
                          className="group flex flex-col justify-between rounded-xl border border-border bg-canvas/40 p-4 text-left transition-colors hover:border-accent hover:bg-surface"
                        >
                          <div>
                            <div className="flex items-center justify-between gap-2">
                              <span className="rounded-full border border-border bg-surface px-2.5 py-0.5 text-[11px] font-medium text-ink-muted">
                                {note.subject}
                              </span>
                              <span className="text-xs text-ink-muted">
                                {t('reading.cardCount', { count: cardCount })}
                              </span>
                            </div>
                            <h3 className="mt-2.5 text-base font-semibold text-ink group-hover:text-accent">
                              {note.title}
                            </h3>
                          </div>

                          {note.outline.length > 0 && (
                            <p className="mt-2.5 line-clamp-2 text-xs text-ink-muted">
                              {note.outline
                                .filter((n) => n.type === 'section')
                                .map((n) => n.heading)
                                .join(' · ')}
                            </p>
                          )}
                        </button>
                      )
                    })}
                  </div>
                )}
              </section>

              {/* Knowledge Cards Index */}
              <section className="space-y-3">
                <div className="flex items-center justify-between">
                  <h2 className="flex items-center gap-2 text-sm font-semibold text-ink">
                    <Layers className="h-4 w-4 text-accent" />
                    <span>{t('reading.cardsHeading')}</span>
                  </h2>
                  <span className="text-xs text-ink-muted">
                    {t('reading.subjectCount', { count: filteredCards.length })}
                  </span>
                </div>

                {loadingCards ? (
                  <div className="py-8 text-center text-sm text-ink-muted">{t('common.loading')}</div>
                ) : filteredCards.length === 0 ? (
                  <div className="rounded-xl border border-border bg-canvas/50 p-8 text-center text-sm text-ink-muted">
                    {t('reading.noCards')}
                  </div>
                ) : (
                  <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
                    {filteredCards.map((card) => (
                      <li key={card.id}>
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
                          className="flex min-h-13 w-full flex-wrap items-center justify-between gap-2 px-4 py-2.5 text-left hover:bg-subtle/60"
                        >
                          <div className="min-w-0 flex-1">
                            <div className="text-sm font-medium text-ink">{card.title}</div>
                            <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-xs text-ink-muted">
                              <span>{card.subject}</span>
                              {card.topics.map((topic) => (
                                <span
                                  key={topic}
                                  className="rounded-full bg-subtle px-2 py-0.5 text-[11px]"
                                >
                                  {topic}
                                </span>
                              ))}
                            </div>
                          </div>
                          <span
                            className={`shrink-0 rounded-full px-2.5 py-0.5 text-[11px] font-medium ${
                              card.verification === 'verified'
                                ? 'bg-ok-soft text-ok'
                                : 'bg-warn-soft text-warn'
                            }`}
                          >
                            {card.verification === 'verified'
                              ? t('common.verified')
                              : t('common.unchecked')}
                          </span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
