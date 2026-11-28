import { Ban, ExternalLink, Info, Puzzle, RefreshCw, Send } from 'lucide-react'
import { useId, useState, type SubmitEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { ApiError } from '../api/client'
import {
  useCancelPluginJob,
  usePluginJobs,
  usePlugins,
  useSubmitPluginJob,
} from '../api/hooks/usePlugins'
import type { PluginJob, PluginJobStatus, PluginSummary } from '../api/types'
import { formatShortTimestamp } from '../triage/proposalHelpers'
import {
  canCancelJob,
  hasActiveJobs,
  isActiveJobStatus,
  parseJobInput,
  progressPercent,
  sortJobsNewestFirst,
} from './settingsLogic'

const MANIFEST_SAMPLE = `{
  "plugins": [
    {
      "name": "mineru",
      "capabilities": ["ingest_pdf"],
      "transport": { "type": "stdio", "command": "/usr/local/bin/reinforce-plugin-mineru" }
    }
  ]
}`

const STATUS_TONE: Record<PluginJobStatus, string> = {
  queued: 'bg-subtle text-ink-muted',
  running: 'bg-accent-soft text-accent',
  succeeded: 'bg-ok-soft text-ok',
  failed: 'bg-bad-soft text-bad',
  cancelled: 'bg-warn-soft text-warn',
}

function errorText(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback
}

export function PluginsSection() {
  const { t } = useTranslation()
  const plugins = usePlugins(true)
  const jobs = usePluginJobs(true)

  return (
    <section
      aria-labelledby="settings-plugins-heading"
      className="space-y-4 rounded-2xl border border-border bg-canvas/40 p-5"
    >
      <header>
        <h2
          id="settings-plugins-heading"
          className="flex items-center gap-2 text-sm font-semibold text-ink"
        >
          <Puzzle className="h-4 w-4 text-accent" />
          <span>{t('settings.plugins.title')}</span>
        </h2>
        <p className="mt-1 text-xs text-ink-muted">{t('settings.plugins.subtitle')}</p>
      </header>

      {plugins.isLoading ? (
        <p className="text-xs text-ink-muted">{t('common.loading')}</p>
      ) : plugins.isError ? (
        <div
          role="alert"
          className="flex items-center justify-between gap-2 rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad"
        >
          <span>
            {t('settings.plugins.loadFailed', {
              message: errorText(plugins.error, t('common.errorGeneric')),
            })}
          </span>
          <button
            type="button"
            onClick={() => void plugins.refetch()}
            className="inline-flex items-center gap-1 font-medium underline"
          >
            <RefreshCw className="h-3 w-3" />
            {t('common.retry')}
          </button>
        </div>
      ) : (plugins.data ?? []).length === 0 ? (
        <EnableInstructions />
      ) : (
        <>
          <PluginList plugins={plugins.data ?? []} />
          <SubmitJobForm plugins={plugins.data ?? []} />
        </>
      )}

      <JobQueue
        jobs={jobs.data}
        isLoading={jobs.isLoading}
        error={jobs.error}
        isFetching={jobs.isFetching}
        onRefresh={() => void jobs.refetch()}
      />
    </section>
  )
}

function EnableInstructions() {
  const { t } = useTranslation()
  return (
    <div className="space-y-2 rounded-xl border border-border bg-surface p-3 text-xs text-ink-muted">
      <p className="flex items-start gap-2 text-ink">
        <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent" />
        <span>{t('settings.plugins.emptyTitle')}</span>
      </p>
      <p>{t('settings.plugins.emptyManifest')}</p>
      <pre className="overflow-auto rounded-lg border border-border bg-canvas/60 p-2.5 font-mono text-[11px] text-ink">
        <code>{MANIFEST_SAMPLE}</code>
      </pre>
      <p>{t('settings.plugins.emptyServe')}</p>
      <pre className="overflow-auto rounded-lg border border-border bg-canvas/60 p-2.5 font-mono text-[11px] text-ink">
        <code>reinforce serve --plugins ./plugins.json</code>
      </pre>
      <p>{t('settings.plugins.emptyBoundary')}</p>
    </div>
  )
}

function PluginList({ plugins }: { plugins: PluginSummary[] }) {
  const { t } = useTranslation()
  return (
    <ul className="grid gap-2 sm:grid-cols-2" aria-label={t('settings.plugins.listLabel')}>
      {plugins.map((plugin) => (
        <li key={plugin.name} className="space-y-2 rounded-xl border border-border bg-surface p-3">
          <div className="flex items-center justify-between gap-2">
            <p className="truncate font-mono text-sm font-semibold text-ink">{plugin.name}</p>
            <span className="rounded-full bg-subtle px-2 py-0.5 text-[10px] uppercase tracking-wide text-ink-muted">
              {plugin.transport}
            </span>
          </div>
          <ul className="flex flex-wrap gap-1" aria-label={t('settings.plugins.capabilities')}>
            {plugin.capabilities.map((capability) => (
              <li
                key={capability}
                className="rounded-md bg-accent-soft px-1.5 py-0.5 font-mono text-[11px] text-accent"
              >
                {capability}
              </li>
            ))}
          </ul>
          <p className="text-[11px] text-ink-muted">
            {t('settings.plugins.limits', {
              timeout: plugin.timeout_secs,
              attempts: plugin.max_attempts,
            })}
          </p>
        </li>
      ))}
    </ul>
  )
}

function SubmitJobForm({ plugins }: { plugins: PluginSummary[] }) {
  const { t } = useTranslation()
  const ids = {
    plugin: useId(),
    capability: useId(),
    input: useId(),
    inputHint: useId(),
    inputError: useId(),
  }
  const submit = useSubmitPluginJob()
  const [pluginName, setPluginName] = useState(plugins[0]?.name ?? '')
  const selected = plugins.find((plugin) => plugin.name === pluginName) ?? plugins[0]
  const [capability, setCapability] = useState(selected?.capabilities[0] ?? '')
  const [inputText, setInputText] = useState('{\n  \n}')
  const [inputError, setInputError] = useState<string | null>(null)
  const [submittedId, setSubmittedId] = useState<string | null>(null)

  const effectiveCapability = selected?.capabilities.includes(capability)
    ? capability
    : (selected?.capabilities[0] ?? '')

  function handlePluginChange(name: string) {
    setPluginName(name)
    const next = plugins.find((plugin) => plugin.name === name)
    setCapability(next?.capabilities[0] ?? '')
  }

  function handleSubmit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault()
    setSubmittedId(null)
    if (!selected || !effectiveCapability) return
    const parsed = parseJobInput(inputText)
    if ('reason' in parsed) {
      setInputError(
        parsed.reason === 'invalid'
          ? t('settings.plugins.inputInvalid', { message: parsed.message ?? '' })
          : t(`settings.plugins.input_${parsed.reason}`),
      )
      return
    }
    setInputError(null)
    submit.mutate(
      { plugin: selected.name, body: { capability: effectiveCapability, input: parsed.value } },
      { onSuccess: (job) => setSubmittedId(job.id) },
    )
  }

  const submitError =
    submit.error instanceof ApiError
      ? `${submit.error.code}: ${submit.error.message}`
      : submit.error
        ? errorText(submit.error, t('common.errorGeneric'))
        : null

  const selectClass =
    'h-10 w-full rounded-xl border border-border bg-surface px-3 text-sm text-ink disabled:opacity-60'

  return (
    <form
      onSubmit={handleSubmit}
      className="space-y-3 rounded-xl border border-border bg-surface/60 p-3"
      aria-labelledby={`${ids.plugin}-legend`}
      noValidate
    >
      <h3 id={`${ids.plugin}-legend`} className="text-xs font-semibold text-ink">
        {t('settings.plugins.submitTitle')}
      </h3>
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1">
          <label htmlFor={ids.plugin} className="text-xs text-ink-muted">
            {t('settings.plugins.plugin')}
          </label>
          <select
            id={ids.plugin}
            value={selected?.name ?? ''}
            onChange={(event) => handlePluginChange(event.target.value)}
            className={selectClass}
          >
            {plugins.map((plugin) => (
              <option key={plugin.name} value={plugin.name}>
                {plugin.name}
              </option>
            ))}
          </select>
        </div>
        <div className="space-y-1">
          <label htmlFor={ids.capability} className="text-xs text-ink-muted">
            {t('settings.plugins.capability')}
          </label>
          <select
            id={ids.capability}
            value={effectiveCapability}
            onChange={(event) => setCapability(event.target.value)}
            disabled={!selected || selected.capabilities.length === 0}
            className={selectClass}
          >
            {selected?.capabilities.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="space-y-1">
        <label htmlFor={ids.input} className="text-xs text-ink-muted">
          {t('settings.plugins.input')}
        </label>
        <textarea
          id={ids.input}
          value={inputText}
          onChange={(event) => {
            setInputText(event.target.value)
            if (inputError) setInputError(null)
          }}
          rows={5}
          spellCheck={false}
          aria-invalid={inputError ? true : undefined}
          aria-describedby={inputError ? `${ids.inputHint} ${ids.inputError}` : ids.inputHint}
          className={`w-full rounded-xl border bg-surface px-3 py-2 font-mono text-xs text-ink ${
            inputError ? 'border-bad' : 'border-border'
          }`}
        />
        <p id={ids.inputHint} className="text-[11px] text-ink-muted">
          {t('settings.plugins.inputHint')}
        </p>
        {inputError && (
          <p id={ids.inputError} role="alert" className="text-xs text-bad">
            {inputError}
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="submit"
          disabled={submit.isPending || !effectiveCapability}
          className="inline-flex h-10 items-center gap-2 rounded-xl bg-accent px-4 text-xs font-semibold text-white hover:bg-accent-hover disabled:opacity-60"
        >
          <Send className="h-3.5 w-3.5" />
          <span>
            {submit.isPending ? t('settings.plugins.submitting') : t('settings.plugins.submit')}
          </span>
        </button>
        {submittedId && (
          <p role="status" className="text-xs text-ok">
            {t('settings.plugins.submitted', { id: submittedId })}
          </p>
        )}
      </div>
      {submitError && (
        <p
          role="alert"
          className="rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad"
        >
          {t('settings.plugins.submitFailed', { message: submitError })}
        </p>
      )}
    </form>
  )
}

interface JobQueueProps {
  jobs: PluginJob[] | undefined
  isLoading: boolean
  isFetching: boolean
  error: unknown
  onRefresh: () => void
}

function JobQueue({ jobs, isLoading, isFetching, error, onRefresh }: JobQueueProps) {
  const { t } = useTranslation()
  const cancel = useCancelPluginJob()
  const polling = hasActiveJobs(jobs)
  const sorted = jobs ? sortJobsNewestFirst(jobs) : []

  return (
    <div className="space-y-2 border-t border-border pt-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 className="text-xs font-semibold text-ink">{t('settings.jobs.title')}</h3>
          <p className="mt-0.5 text-[11px] text-ink-muted" aria-live="polite">
            {polling ? t('settings.jobs.polling') : t('settings.jobs.idle')}
          </p>
        </div>
        <button
          type="button"
          onClick={onRefresh}
          disabled={isFetching}
          className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-surface px-2.5 text-xs text-ink-muted hover:text-ink disabled:opacity-60"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${isFetching ? 'animate-spin' : ''}`} />
          <span>{t('settings.jobs.refresh')}</span>
        </button>
      </div>

      {cancel.isError && (
        <p
          role="alert"
          className="rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad"
        >
          {t('settings.jobs.cancelFailed', {
            message: errorText(cancel.error, t('common.errorGeneric')),
          })}
        </p>
      )}

      {isLoading ? (
        <p className="text-xs text-ink-muted">{t('common.loading')}</p>
      ) : error ? (
        <p
          role="alert"
          className="rounded-xl border border-bad/40 bg-bad-soft px-3 py-2 text-xs text-bad"
        >
          {t('settings.jobs.loadFailed', { message: errorText(error, t('common.errorGeneric')) })}
        </p>
      ) : sorted.length === 0 ? (
        <p className="text-xs text-ink-muted">{t('settings.jobs.empty')}</p>
      ) : (
        <ul className="space-y-2" aria-label={t('settings.jobs.listLabel')}>
          {sorted.map((job) => (
            <JobRow
              key={job.id}
              job={job}
              cancelling={cancel.isPending && cancel.variables === job.id}
              onCancel={() => cancel.mutate(job.id)}
            />
          ))}
        </ul>
      )}
    </div>
  )
}

function JobRow({
  job,
  cancelling,
  onCancel,
}: {
  job: PluginJob
  cancelling: boolean
  onCancel: () => void
}) {
  const { t } = useTranslation()
  const percent = progressPercent(job.progress)
  const active = isActiveJobStatus(job.status)
  const title = `${job.plugin} · ${job.capability}`

  return (
    <li className="space-y-2 rounded-xl border border-border bg-surface p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate font-mono text-xs font-semibold text-ink">{title}</p>
          <p className="mt-0.5 text-[11px] text-ink-muted">
            {t('settings.jobs.meta', {
              time: formatShortTimestamp(job.created_at),
              attempts: job.attempts,
              max: job.max_attempts,
            })}
          </p>
        </div>
        <div className="flex items-center gap-2">
          {job.cancel_requested && active && (
            <span className="rounded-full bg-warn-soft px-2 py-0.5 text-[10px] text-warn">
              {t('settings.jobs.cancelRequested')}
            </span>
          )}
          <span
            className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${STATUS_TONE[job.status]}`}
          >
            {t(`settings.jobs.status.${job.status}`)}
          </span>
          {canCancelJob(job) && (
            <button
              type="button"
              onClick={onCancel}
              disabled={cancelling}
              aria-label={t('settings.jobs.cancelNamed', { name: title })}
              className="inline-flex h-7 items-center gap-1 rounded-lg border border-border px-2 text-[11px] text-ink-muted hover:border-bad/50 hover:text-bad disabled:opacity-60"
            >
              <Ban className="h-3 w-3" />
              <span>{cancelling ? t('settings.jobs.cancelling') : t('common.cancel')}</span>
            </button>
          )}
        </div>
      </div>

      {active && (
        <div className="space-y-1">
          <div
            role="progressbar"
            aria-label={t('settings.jobs.progressLabel', { name: title })}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
            className="h-1.5 overflow-hidden rounded-full bg-subtle"
          >
            <div
              className="h-full rounded-full bg-accent transition-[width] duration-500"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="text-[11px] text-ink-muted">
            {percent}%{job.progress_message ? ` · ${job.progress_message}` : ''}
          </p>
        </div>
      )}

      {job.error && (
        <p className="rounded-lg bg-bad-soft px-2.5 py-1.5 text-[11px] text-bad">
          <span className="font-mono font-semibold">{job.error.code}</span>
          {job.error.message ? ` — ${job.error.message}` : ''}
        </p>
      )}

      {job.proposal_id && (
        <Link
          to={`/triage/${encodeURIComponent(job.proposal_id)}`}
          className="inline-flex items-center gap-1 text-xs font-medium text-accent hover:underline"
        >
          <ExternalLink className="h-3 w-3" />
          <span>{t('settings.jobs.viewProposal')}</span>
        </Link>
      )}
    </li>
  )
}
