import { BookOpen, Flame, Layers, Play, Search, Sparkles, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router'
import { ApiError } from '../api/client'
import { useKnowledgeCards } from '../api/hooks/useKnowledgeCards'
import { useNotes } from '../api/hooks/useNotes'
import { useSearch } from '../api/hooks/useQuestions'
import { useCreateSession } from '../api/hooks/useStudySession'
import type { OutlineNode, SessionMode } from '../api/types'

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
  const navigate = useNavigate()
  const [selectedSubject, setSelectedSubject] = useState<string>('')
  const [searchInput, setSearchInput] = useState('')
  const [launchError, setLaunchError] = useState<string | null>(null)

  const { data: notes = [], isLoading: loadingNotes, error: notesError } = useNotes()
  const { data: cards = [], isLoading: loadingCards } = useKnowledgeCards()
  const { data: searchResults = [], isFetching: searching } = useSearch(searchInput)
  const createSession = useCreateSession()

  const subjects = useMemo(() => {
    const set = new Set<string>()
    for (const n of notes) set.add(n.subject)
    for (const c of cards) set.add(c.subject)
    return Array.from(set).sort()
  }, [notes, cards])

  const filteredNotes = useMemo(
    () => (selectedSubject ? notes.filter((n) => n.subject === selectedSubject) : notes),
    [notes, selectedSubject],
  )

  const filteredCards = useMemo(
    () => (selectedSubject ? cards.filter((c) => c.subject === selectedSubject) : cards),
    [cards, selectedSubject],
  )

  async function quickStart(mode: SessionMode) {
    setLaunchError(null)
    try {
      const res = await createSession.mutateAsync({ mode })
      if (res.question_count === 0) {
        setLaunchError(t('study.options.emptySessionError'))
        return
      }
      void navigate(`/study/session/${encodeURIComponent(res.session_id)}`, {
        state: { mode },
      })
    } catch (err) {
      setLaunchError(err instanceof ApiError ? err.message : t('common.errorGeneric'))
    }
  }

  const hasSearch = searchInput.trim().length > 0

  return (
    <div className="mx-auto max-w-5xl space-y-6 px-4 py-5 sm:px-6 lg:py-8">
      {/* Daily Due Entry Strip (not a marketing hero — direct study action) */}
      <section className="flex flex-col justify-between gap-4 rounded-xl border border-border bg-surface p-4 sm:flex-row sm:items-center sm:p-5">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-base font-semibold text-ink">
            <Sparkles className="h-4 w-4 text-accent" />
            <h1>{t('reading.dailyDueBannerTitle')}</h1>
          </div>
          <p className="mt-0.5 text-sm text-ink-muted">{t('reading.dailyDueBannerDesc')}</p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={createSession.isPending}
            onClick={() => void quickStart('daily_due')}
            className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50"
          >
            <Play className="h-4 w-4 fill-current" />
            <span>{t('reading.startDailyDue')}</span>
          </button>

          <button
            type="button"
            disabled={createSession.isPending}
            onClick={() => void quickStart('weakness_drill')}
            className="inline-flex min-h-11 items-center gap-2 rounded-lg border border-border bg-canvas px-4 text-sm font-medium text-ink hover:bg-subtle disabled:opacity-50"
          >
            <Flame className="h-4 w-4 text-warn" />
            <span>{t('reading.startWeakness')}</span>
          </button>
        </div>
      </section>

      {launchError && (
        <div className="rounded-lg border border-warn/40 bg-warn-soft px-4 py-3 text-sm text-warn">
          {launchError}
        </div>
      )}

      {/* Search & Subject Filter */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-ink-muted" />
          <input
            type="search"
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
            placeholder={t('common.search')}
            className="min-h-11 w-full rounded-lg border border-border bg-surface pr-10 pl-10 text-sm text-ink placeholder:text-ink-muted"
          />
          {hasSearch && (
            <button
              type="button"
              onClick={() => setSearchInput('')}
              aria-label={t('common.close')}
              className="absolute top-1/2 right-1 flex h-9 w-9 -translate-y-1/2 items-center justify-center rounded-md text-ink-muted hover:bg-subtle"
            >
              <X className="h-4 w-4" />
            </button>
          )}
        </div>

        {subjects.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <button
              type="button"
              onClick={() => setSelectedSubject('')}
              className={`min-h-11 rounded-lg border px-3 text-xs font-medium transition-colors ${
                selectedSubject === ''
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
                onClick={() => setSelectedSubject(subj)}
                className={`min-h-11 rounded-lg border px-3 text-xs font-medium transition-colors ${
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

      {/* Live Search Results */}
      {hasSearch ? (
        <section className="space-y-3">
          <h2 className="text-sm font-semibold text-ink-muted">
            {t('reading.searchResultsHeading')}
          </h2>

          {searching ? (
            <div className="py-8 text-center text-sm text-ink-muted">{t('common.loading')}</div>
          ) : searchResults.length === 0 ? (
            <div className="rounded-xl border border-border bg-surface p-8 text-center text-sm text-ink-muted">
              {t('reading.noSearchResults')}
            </div>
          ) : (
            <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
              {searchResults.map((hit) => {
                if (hit.kind === 'note') {
                  return (
                    <li key={`note-${hit.id}`}>
                      <Link
                        to={`/notes/${encodeURIComponent(hit.id)}`}
                        className="flex min-h-12 items-center justify-between gap-3 px-4 py-3 hover:bg-subtle/60"
                      >
                        <span className="font-medium text-ink">{hit.title}</span>
                        <span className="rounded bg-subtle px-2 py-0.5 text-xs text-ink-muted">
                          {t('reading.notesHeading')}
                        </span>
                      </Link>
                    </li>
                  )
                }
                if (hit.kind === 'knowledge_card') {
                  return (
                    <li key={`card-${hit.id}`}>
                      <Link
                        to={`/cards/${encodeURIComponent(hit.id)}`}
                        className="flex min-h-12 items-center justify-between gap-3 px-4 py-3 hover:bg-subtle/60"
                      >
                        <span className="font-medium text-ink">{hit.title}</span>
                        <span className="rounded bg-accent-soft px-2 py-0.5 text-xs text-accent">
                          {t('reading.cardsHeading')}
                        </span>
                      </Link>
                    </li>
                  )
                }
                const previewText = hit.title
                  .map((b) => (b.type === 'markdown' ? b.text : b.type === 'math' ? b.latex : ''))
                  .join(' ')
                  .slice(0, 120)
                return (
                  <li key={`q-${hit.id}`} className="flex min-h-12 items-center justify-between gap-3 px-4 py-3">
                    <span className="truncate text-sm text-ink">{previewText || hit.id}</span>
                    <span className="shrink-0 rounded bg-subtle px-2 py-0.5 text-xs text-ink-muted">
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
            <div className="rounded-lg bg-bad-soft p-4 text-sm text-bad">
              {notesError instanceof Error ? notesError.message : t('common.errorGeneric')}
            </div>
          )}

          {/* Notes Section */}
          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="flex items-center gap-2 text-base font-semibold text-ink">
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
              <div className="rounded-xl border border-border bg-surface p-8 text-center text-sm text-ink-muted">
                {t('reading.noNotes')}
              </div>
            ) : (
              <div className="grid gap-3 sm:grid-cols-2">
                {filteredNotes.map((note) => {
                  const cardCount = countCardsInOutline(note.outline)
                  return (
                    <Link
                      key={note.id}
                      to={`/notes/${encodeURIComponent(note.id)}`}
                      className="group flex flex-col justify-between rounded-xl border border-border bg-surface p-4 transition-colors hover:border-accent"
                    >
                      <div>
                        <div className="flex items-center justify-between gap-2">
                          <span className="rounded bg-subtle px-2 py-0.5 text-xs font-medium text-ink-muted">
                            {note.subject}
                          </span>
                          <span className="text-xs text-ink-muted">
                            {t('reading.cardCount', { count: cardCount })}
                          </span>
                        </div>
                        <h3 className="mt-2 text-base font-semibold text-ink group-hover:text-accent">
                          {note.title}
                        </h3>
                      </div>

                      {note.outline.length > 0 && (
                        <p className="mt-3 line-clamp-2 text-xs text-ink-muted">
                          {note.outline
                            .filter((n) => n.type === 'section')
                            .map((n) => n.heading)
                            .join(' / ')}
                        </p>
                      )}
                    </Link>
                  )
                })}
              </div>
            )}
          </section>

          {/* Knowledge Cards Index */}
          <section className="space-y-3">
            <div className="flex items-center justify-between">
              <h2 className="flex items-center gap-2 text-base font-semibold text-ink">
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
              <div className="rounded-xl border border-border bg-surface p-8 text-center text-sm text-ink-muted">
                {t('reading.noCards')}
              </div>
            ) : (
              <ul className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-surface">
                {filteredCards.map((card) => (
                  <li key={card.id}>
                    <Link
                      to={`/cards/${encodeURIComponent(card.id)}`}
                      className="flex min-h-14 flex-wrap items-center justify-between gap-2 px-4 py-3 hover:bg-subtle/60"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="font-medium text-ink">{card.title}</div>
                        <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-xs text-ink-muted">
                          <span>{card.subject}</span>
                          {card.topics.map((topic) => (
                            <span key={topic} className="rounded bg-subtle px-1.5 py-0.5">
                              {topic}
                            </span>
                          ))}
                        </div>
                      </div>
                      <span
                        className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium ${
                          card.verification === 'verified'
                            ? 'bg-ok-soft text-ok'
                            : 'bg-warn-soft text-warn'
                        }`}
                      >
                        {card.verification === 'verified'
                          ? t('common.verified')
                          : t('common.unchecked')}
                      </span>
                    </Link>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </>
      )}
    </div>
  )
}
