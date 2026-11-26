import {
  AlertTriangle,
  BookOpen,
  CheckCircle2,
  Download,
  ExternalLink,
  FileArchive,
  FileOutput,
  FileText,
  Flame,
  FolderOpen,
  Loader2,
  PenTool,
  Sparkles,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError } from '../api/client'
import { useExportContent } from '../api/hooks/useExport'
import { useKnowledgeCards } from '../api/hooks/useKnowledgeCards'
import { useNotes } from '../api/hooks/useNotes'
import { useWeaknessReport } from '../api/hooks/useStudySession'
import type { ExportFormat, ExportPreset, ExportRequest, ExportScope } from '../api/types'

type ScopeKind = ExportScope['kind']

const PRESETS: ExportPreset[] = ['standard', 'goodnotes', 'worksheet']

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

function triggerDownload(url: string, filename: string) {
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.rel = 'noreferrer'
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
}

interface GeneratedArtifact {
  format: ExportFormat
  url: string
  filename: string
  sizeBytes: number
}

export function ExportCenterPage() {
  const { t } = useTranslation()
  const { data: notes = [], isLoading: loadingNotes } = useNotes()
  const { data: cards = [] } = useKnowledgeCards()
  const { data: weakness } = useWeaknessReport(100)
  const exportMutation = useExportContent()

  const [scopeKind, setScopeKind] = useState<ScopeKind>('note')
  const [selectedNoteId, setSelectedNoteId] = useState<string>('')
  const [selectedSubject, setSelectedSubject] = useState<string>('')
  const [threshold, setThreshold] = useState<number>(0.35)
  const [format, setFormat] = useState<ExportFormat>('pdf')
  const [preset, setPreset] = useState<ExportPreset>('standard')
  const [webBaseUrl, setWebBaseUrl] = useState<string>(() =>
    typeof window !== 'undefined' ? window.location.origin : '',
  )

  const [artifact, setArtifact] = useState<GeneratedArtifact | null>(null)
  const [errorState, setErrorState] = useState<{ kind: 'warn' | 'bad'; message: string } | null>(
    null,
  )
  const activeUrlRef = useRef<string | null>(null)

  useEffect(() => {
    return () => {
      if (activeUrlRef.current) {
        URL.revokeObjectURL(activeUrlRef.current)
        activeUrlRef.current = null
      }
    }
  }, [])

  const subjects = useMemo(() => {
    const set = new Set<string>()
    for (const n of notes) set.add(n.subject)
    for (const c of cards) set.add(c.subject)
    return Array.from(set).sort()
  }, [notes, cards])

  const effectiveNoteId =
    selectedNoteId && notes.some((n) => n.id === selectedNoteId)
      ? selectedNoteId
      : (notes[0]?.id ?? '')

  const effectiveSubject =
    selectedSubject && subjects.includes(selectedSubject)
      ? selectedSubject
      : (subjects[0] ?? '')

  const weaknessStats = useMemo(() => {
    if (!weakness) return { cards: 0, questions: 0 }
    return {
      cards: weakness.knowledge_cards.filter((c) => c.score >= threshold).length,
      questions: weakness.questions.filter((q) => q.score >= threshold).length,
    }
  }, [weakness, threshold])

  const canSubmit =
    !exportMutation.isPending &&
    ((scopeKind === 'note' && Boolean(effectiveNoteId)) ||
      (scopeKind === 'subject' && Boolean(effectiveSubject)) ||
      scopeKind === 'weakness')

  function buildRequest(): ExportRequest | null {
    let scope: ExportScope
    if (scopeKind === 'note') {
      if (!effectiveNoteId) return null
      scope = { kind: 'note', note_id: effectiveNoteId }
    } else if (scopeKind === 'subject') {
      if (!effectiveSubject) return null
      scope = { kind: 'subject', subject: effectiveSubject }
    } else {
      scope = { kind: 'weakness', threshold }
    }

    const req: ExportRequest = {
      format,
      scope,
    }
    if (format === 'pdf') {
      req.preset = preset
    }
    const trimmedUrl = webBaseUrl.trim()
    if (trimmedUrl) {
      req.web_base_url = trimmedUrl
    }
    return req
  }

  async function handleExport() {
    const req = buildRequest()
    if (!req) return
    setErrorState(null)

    try {
      const res = await exportMutation.mutateAsync(req)
      if (activeUrlRef.current) {
        URL.revokeObjectURL(activeUrlRef.current)
      }
      const nextUrl = URL.createObjectURL(res.blob)
      activeUrlRef.current = nextUrl

      const nextArtifact: GeneratedArtifact = {
        format: req.format,
        url: nextUrl,
        filename: res.filename,
        sizeBytes: res.blob.size,
      }
      setArtifact(nextArtifact)

      if (req.format === 'markdown') {
        triggerDownload(nextUrl, res.filename)
      }
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === 'empty_export') {
          setErrorState({ kind: 'warn', message: t('export.errors.emptyExport') })
          return
        }
        if (err.code === 'fonts_missing') {
          setErrorState({ kind: 'warn', message: t('export.errors.fontsMissing') })
          return
        }
        if (err.status === 404) {
          setErrorState({ kind: 'warn', message: t('export.errors.serverUpdateRequired') })
          return
        }
        setErrorState({ kind: 'bad', message: err.message })
        return
      }
      setErrorState({ kind: 'bad', message: t('common.errorGeneric') })
    }
  }

  return (
    <div className="mx-auto max-w-6xl space-y-6 px-4 py-5 sm:px-6 lg:py-8">
      {/* Header */}
      <section className="rounded-xl border border-border bg-surface p-4 sm:p-5">
        <div className="flex items-center gap-2 text-lg font-semibold text-ink">
          <FileOutput className="h-5 w-5 text-accent" />
          <h1>{t('export.title')}</h1>
        </div>
        <p className="mt-0.5 text-sm text-ink-muted">{t('export.subtitle')}</p>
      </section>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)] lg:items-start">
        {/* Left: Configuration form */}
        <section className="space-y-5 rounded-xl border border-border bg-surface p-4 sm:p-5">
          {/* 1. Scope */}
          <div className="space-y-3">
            <h2 className="text-sm font-semibold text-ink">{t('export.scopeHeading')}</h2>

            <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
              {(
                [
                  { kind: 'note', icon: BookOpen, title: t('export.scopeNote'), desc: t('export.scopeNoteDesc') },
                  { kind: 'subject', icon: FolderOpen, title: t('export.scopeSubject'), desc: t('export.scopeSubjectDesc') },
                  { kind: 'weakness', icon: Flame, title: t('export.scopeWeakness'), desc: t('export.scopeWeaknessDesc') },
                ] as const
              ).map(({ kind, icon: Icon, title, desc }) => {
                const active = scopeKind === kind
                return (
                  <button
                    key={kind}
                    type="button"
                    aria-pressed={active}
                    onClick={() => setScopeKind(kind)}
                    className={`flex min-h-14 flex-col items-start rounded-xl border p-3 text-left transition-colors ${
                      active
                        ? 'border-accent bg-accent-soft text-accent'
                        : 'border-border bg-canvas text-ink hover:bg-subtle'
                    }`}
                  >
                    <div className="flex items-center gap-1.5 text-xs font-semibold">
                      <Icon className="h-4 w-4 shrink-0" />
                      <span>{title}</span>
                    </div>
                    <span className="mt-1 text-[11px] leading-snug text-ink-muted">{desc}</span>
                  </button>
                )
              })}
            </div>

            {/* Scope details */}
            {scopeKind === 'note' && (
              <div>
                <label htmlFor="export-note-select" className="block text-xs font-medium text-ink">
                  {t('export.selectNote')}
                </label>
                {loadingNotes ? (
                  <div className="mt-1.5 text-xs text-ink-muted">{t('common.loading')}</div>
                ) : notes.length === 0 ? (
                  <div className="mt-1.5 rounded-lg bg-subtle px-3 py-2 text-xs text-ink-muted">
                    {t('export.noNotes')}
                  </div>
                ) : (
                  <select
                    id="export-note-select"
                    value={effectiveNoteId}
                    onChange={(e) => setSelectedNoteId(e.target.value)}
                    className="mt-1.5 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
                  >
                    {notes.map((n) => (
                      <option key={n.id} value={n.id}>
                        [{n.subject}] {n.title}
                      </option>
                    ))}
                  </select>
                )}
              </div>
            )}

            {scopeKind === 'subject' && (
              <div>
                <label className="block text-xs font-medium text-ink">
                  {t('export.selectSubject')}
                </label>
                {subjects.length === 0 ? (
                  <div className="mt-1.5 rounded-lg bg-subtle px-3 py-2 text-xs text-ink-muted">
                    {t('export.noSubjects')}
                  </div>
                ) : (
                  <div className="mt-1.5 flex flex-wrap gap-2">
                    {subjects.map((subj) => (
                      <button
                        key={subj}
                        type="button"
                        onClick={() => setSelectedSubject(subj)}
                        className={`min-h-11 rounded-lg border px-3.5 text-xs font-medium transition-colors ${
                          effectiveSubject === subj
                            ? 'border-accent bg-accent-soft text-accent'
                            : 'border-border bg-canvas text-ink-muted hover:text-ink'
                        }`}
                      >
                        {subj}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}

            {scopeKind === 'weakness' && (
              <div className="space-y-2 rounded-lg border border-border bg-subtle/40 p-3.5">
                <div className="flex items-center justify-between text-xs">
                  <label htmlFor="export-weakness-range" className="font-medium text-ink">
                    {t('export.weaknessThreshold')}
                  </label>
                  <span className="rounded bg-accent-soft px-2 py-0.5 font-mono font-semibold text-accent">
                    {Math.round(threshold * 100)}% ({threshold.toFixed(2)})
                  </span>
                </div>
                <input
                  id="export-weakness-range"
                  type="range"
                  min={0}
                  max={1}
                  step={0.05}
                  value={threshold}
                  onChange={(e) => setThreshold(Number(e.target.value))}
                  className="h-9 w-full cursor-pointer accent-accent"
                />
                <p className="text-xs text-ink-muted">
                  {t('export.weaknessMatchSummary', {
                    cards: weaknessStats.cards,
                    questions: weaknessStats.questions,
                  })}
                </p>
              </div>
            )}
          </div>

          <div className="border-t border-border" />

          {/* 2. Format & Preset */}
          <div className="space-y-3">
            <h2 className="text-sm font-semibold text-ink">{t('export.formatHeading')}</h2>

            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              <button
                type="button"
                aria-pressed={format === 'pdf'}
                onClick={() => setFormat('pdf')}
                className={`flex min-h-14 flex-col items-start rounded-xl border p-3 text-left transition-colors ${
                  format === 'pdf'
                    ? 'border-accent bg-accent-soft text-accent'
                    : 'border-border bg-canvas text-ink hover:bg-subtle'
                }`}
              >
                <div className="flex items-center gap-1.5 text-sm font-semibold">
                  <FileText className="h-4 w-4 shrink-0" />
                  <span>{t('export.formatPdf')}</span>
                </div>
                <span className="mt-1 text-xs text-ink-muted">{t('export.formatPdfDesc')}</span>
              </button>

              <button
                type="button"
                aria-pressed={format === 'markdown'}
                onClick={() => setFormat('markdown')}
                className={`flex min-h-14 flex-col items-start rounded-xl border p-3 text-left transition-colors ${
                  format === 'markdown'
                    ? 'border-accent bg-accent-soft text-accent'
                    : 'border-border bg-canvas text-ink hover:bg-subtle'
                }`}
              >
                <div className="flex items-center gap-1.5 text-sm font-semibold">
                  <FileArchive className="h-4 w-4 shrink-0" />
                  <span>{t('export.formatMarkdown')}</span>
                </div>
                <span className="mt-1 text-xs text-ink-muted">{t('export.formatMarkdownDesc')}</span>
              </button>
            </div>

            {format === 'pdf' && (
              <div className="space-y-2 pt-1">
                <div className="text-xs font-medium text-ink">{t('export.presetHeading')}</div>
                <div className="space-y-2">
                  {PRESETS.map((p) => {
                    const active = preset === p
                    const Icon = p === 'goodnotes' ? PenTool : p === 'worksheet' ? Sparkles : FileText
                    return (
                      <button
                        key={p}
                        type="button"
                        aria-pressed={active}
                        onClick={() => setPreset(p)}
                        className={`flex w-full items-start gap-3 rounded-xl border p-3 text-left transition-colors ${
                          active
                            ? 'border-accent bg-accent-soft'
                            : 'border-border bg-canvas hover:bg-subtle'
                        }`}
                      >
                        <Icon
                          className={`mt-0.5 h-4 w-4 shrink-0 ${active ? 'text-accent' : 'text-ink-muted'}`}
                        />
                        <div className="min-w-0">
                          <div className={`text-sm font-semibold ${active ? 'text-accent' : 'text-ink'}`}>
                            {t(`export.presets.${p}.name`)}
                          </div>
                          <div className="mt-0.5 text-xs text-ink-muted">
                            {t(`export.presets.${p}.desc`)}
                          </div>
                        </div>
                      </button>
                    )
                  })}
                </div>
              </div>
            )}
          </div>

          {/* Optional web_base_url */}
          <div className="space-y-1.5">
            <label htmlFor="export-web-base-url" className="block text-xs font-medium text-ink">
              {t('export.deepLinkHeading')}
            </label>
            <input
              id="export-web-base-url"
              type="url"
              value={webBaseUrl}
              onChange={(e) => setWebBaseUrl(e.target.value)}
              placeholder={t('export.deepLinkPlaceholder')}
              className="min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-xs text-ink placeholder:text-ink-muted"
            />
            <p className="text-[11px] text-ink-muted">{t('export.deepLinkHint')}</p>
          </div>

          {errorState && (
            <div
              role="alert"
              className={`flex items-start gap-2 rounded-xl border px-3.5 py-3 text-xs ${
                errorState.kind === 'warn'
                  ? 'border-warn/40 bg-warn-soft text-warn'
                  : 'border-bad/40 bg-bad-soft text-bad'
              }`}
            >
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{errorState.message}</span>
            </div>
          )}

          <button
            type="button"
            disabled={!canSubmit}
            onClick={() => void handleExport()}
            className="inline-flex min-h-12 w-full items-center justify-center gap-2 rounded-xl bg-accent px-5 text-sm font-semibold text-white hover:bg-accent-hover disabled:opacity-45"
          >
            {exportMutation.isPending ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                <span>{t('export.generating')}</span>
              </>
            ) : format === 'pdf' ? (
              <>
                <FileText className="h-4 w-4" />
                <span>{t('export.generatePdf')}</span>
              </>
            ) : (
              <>
                <Download className="h-4 w-4" />
                <span>{t('export.downloadMarkdown')}</span>
              </>
            )}
          </button>
        </section>

        {/* Right: Live PDF Preview / Download Ready Pane */}
        <section className="flex flex-col rounded-xl border border-border bg-surface">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-3">
            <h2 className="text-sm font-semibold text-ink">{t('export.previewHeading')}</h2>

            {artifact?.format === 'pdf' && (
              <div className="flex flex-wrap items-center gap-2">
                <a
                  href={artifact.url}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex min-h-10 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-ink hover:bg-subtle"
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                  <span>{t('export.openPdfTab')}</span>
                </a>
                <button
                  type="button"
                  onClick={() => triggerDownload(artifact.url, artifact.filename)}
                  className="inline-flex min-h-10 items-center gap-1.5 rounded-lg bg-accent px-3 text-xs font-medium text-white hover:bg-accent-hover"
                >
                  <Download className="h-3.5 w-3.5" />
                  <span>{t('export.downloadPdf')}</span>
                </button>
              </div>
            )}
          </div>

          <div className="p-4">
            {!artifact ? (
              <div className="flex min-h-[45dvh] flex-col items-center justify-center rounded-xl border border-dashed border-border bg-subtle/30 p-8 text-center text-sm text-ink-muted">
                <FileOutput className="mb-2 h-8 w-8 opacity-40" />
                <p className="max-w-xs">{t('export.previewEmpty')}</p>
              </div>
            ) : artifact.format === 'pdf' ? (
              <div className="space-y-2">
                <div className="flex items-center justify-between text-xs text-ink-muted">
                  <span className="truncate font-mono">{artifact.filename}</span>
                  <span>{formatBytes(artifact.sizeBytes)}</span>
                </div>
                <div className="overflow-hidden rounded-xl border border-border bg-subtle">
                  <iframe
                    src={artifact.url}
                    title={artifact.filename}
                    className="h-[65dvh] w-full"
                  />
                </div>
              </div>
            ) : (
              <div className="flex min-h-[30dvh] flex-col items-center justify-center gap-4 rounded-xl border border-ok/40 bg-ok-soft/50 p-8 text-center">
                <CheckCircle2 className="h-10 w-10 text-ok" />
                <div className="text-sm font-medium text-ink">
                  {t('export.markdownReady', {
                    filename: artifact.filename,
                    size: formatBytes(artifact.sizeBytes),
                  })}
                </div>
                <button
                  type="button"
                  onClick={() => triggerDownload(artifact.url, artifact.filename)}
                  className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover"
                >
                  <Download className="h-4 w-4" />
                  <span>{t('export.downloadAgain')}</span>
                </button>
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  )
}
