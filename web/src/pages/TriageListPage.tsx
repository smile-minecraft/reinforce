import {
  CheckCircle2,
  ChevronRight,
  ClipboardCheck,
  Clock,
  Lock,
  MapPin,
  MessageSquareWarning,
  Play,
  XCircle,
} from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation, useNavigate } from 'react-router'
import { ApiError } from '../api/client'
import { useAuthStatus } from '../api/hooks/useAuth'
import { useProposals } from '../api/hooks/useProposals'
import type { ProposalStatus } from '../api/types'
import {
  formatShortTimestamp,
  getProposalOpCounts,
  getProposalOperations,
  getProposalSummaryTitle,
  proposalHasInteractivePins,
} from '../triage/proposalHelpers'

const STATUS_TABS: ProposalStatus[] = ['pending', 'approved', 'rejected']

export function TriageListPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const location = useLocation()
  const navState = location.state as { notice?: string } | null
  const [status, setStatus] = useState<ProposalStatus>('pending')

  const { data: authStatus } = useAuthStatus()
  const isNonOwner = Boolean(authStatus?.role && authStatus.role !== 'owner')

  const { data: proposals = [], isLoading, error, refetch } = useProposals(status)

  const forbidden =
    isNonOwner || (error instanceof ApiError && error.status === 403)

  if (forbidden) {
    return (
      <div className="mx-auto max-w-3xl px-4 py-10 sm:px-6">
        <div className="rounded-xl border border-warn/40 bg-warn-soft p-6 text-center">
          <Lock className="mx-auto h-8 w-8 text-warn" />
          <h1 className="mt-3 text-base font-semibold text-ink">{t('triage.ownerOnlyTitle')}</h1>
          <p className="mt-1 text-sm text-ink-muted">{t('triage.ownerOnlyDesc')}</p>
        </div>
      </div>
    )
  }

  return (
    <div className="mx-auto max-w-5xl space-y-6 px-4 py-5 sm:px-6 lg:py-8">
      {/* Header */}
      <section className="flex flex-col justify-between gap-4 rounded-xl border border-border bg-surface p-4 sm:flex-row sm:items-center sm:p-5">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-lg font-semibold text-ink">
            <ClipboardCheck className="h-5 w-5 text-accent" />
            <h1>{t('triage.title')}</h1>
          </div>
          <p className="mt-0.5 text-sm text-ink-muted">{t('triage.subtitle')}</p>
        </div>

        {status === 'pending' && proposals.length > 0 && (
          <button
            type="button"
            onClick={() => void navigate(`/triage/${encodeURIComponent(proposals[0].id)}`)}
            className="inline-flex min-h-11 shrink-0 items-center justify-center gap-2 rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover"
          >
            <Play className="h-4 w-4 fill-current" />
            <span>{t('triage.startTriaging')}</span>
          </button>
        )}
      </section>

      {navState?.notice && (
        <div className="flex items-center gap-2 rounded-xl border border-ok/40 bg-ok-soft px-4 py-3 text-sm font-medium text-ok">
          <CheckCircle2 className="h-4 w-4 shrink-0" />
          <span>{navState.notice}</span>
        </div>
      )}

      {/* Status tabs */}
      <div className="flex flex-wrap items-center gap-2" role="tablist" aria-label={t('triage.title')}>
        {STATUS_TABS.map((tab) => {
          const active = status === tab
          return (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => setStatus(tab)}
              className={`inline-flex min-h-11 items-center gap-2 rounded-lg border px-4 text-sm font-medium transition-colors ${
                active
                  ? 'border-accent bg-accent-soft text-accent'
                  : 'border-border bg-surface text-ink-muted hover:bg-subtle hover:text-ink'
              }`}
            >
              {tab === 'pending' && <Clock className="h-4 w-4" />}
              {tab === 'approved' && <CheckCircle2 className="h-4 w-4" />}
              {tab === 'rejected' && <XCircle className="h-4 w-4" />}
              <span>{t(`triage.status.${tab}`)}</span>
              {active && !isLoading && (
                <span className="rounded-full bg-accent/15 px-2 py-0.5 font-mono text-xs">
                  {proposals.length}
                </span>
              )}
            </button>
          )
        })}
      </div>

      {/* Proposal list */}
      {isLoading ? (
        <div className="py-12 text-center text-sm text-ink-muted">{t('common.loading')}</div>
      ) : error ? (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-bad/40 bg-bad-soft p-4 text-sm text-bad">
          <span>{error instanceof Error ? error.message : t('common.errorGeneric')}</span>
          <button
            type="button"
            onClick={() => void refetch()}
            className="min-h-10 rounded-lg border border-bad/40 bg-surface px-3 text-xs font-medium text-bad hover:bg-bad-soft"
          >
            {t('common.retry')}
          </button>
        </div>
      ) : proposals.length === 0 ? (
        <div className="rounded-xl border border-border bg-surface p-10 text-center text-sm text-ink-muted">
          {status === 'pending'
            ? t('triage.emptyPending')
            : status === 'approved'
              ? t('triage.emptyApproved')
              : t('triage.emptyRejected')}
        </div>
      ) : (
        <ul className="space-y-3">
          {proposals.map((proposal) => {
            const title = getProposalSummaryTitle(proposal)
            const opCounts = getProposalOpCounts(proposal)
            const totalOps = getProposalOperations(proposal).length
            const hasPins = proposalHasInteractivePins(proposal)

            return (
              <li key={proposal.id}>
                <Link
                  to={`/triage/${encodeURIComponent(proposal.id)}`}
                  className="group flex flex-col gap-3 rounded-xl border border-border bg-surface p-4 transition-colors hover:border-accent/60 hover:bg-subtle/30 sm:flex-row sm:items-center sm:justify-between"
                >
                  <div className="min-w-0 flex-1 space-y-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <span
                        className={`rounded-md px-2 py-0.5 text-xs font-medium ${
                          proposal.status === 'pending'
                            ? 'bg-warn-soft text-warn'
                            : proposal.status === 'approved'
                              ? 'bg-ok-soft text-ok'
                              : 'bg-bad-soft text-bad'
                        }`}
                      >
                        {t(`triage.status.${proposal.status}`)}
                      </span>
                      <span className="text-xs text-ink-muted">
                        {t('triage.opCount', { count: totalOps })}
                      </span>
                      {hasPins && (
                        <span className="inline-flex items-center gap-1 rounded-md bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">
                          <MapPin className="h-3 w-3" />
                          {t('triage.hasPins')}
                        </span>
                      )}
                    </div>

                    <h2 className="truncate text-base font-semibold text-ink group-hover:text-accent">
                      {title}
                    </h2>

                    <div className="flex flex-wrap items-center gap-1.5 text-xs text-ink-muted">
                      {opCounts.map(({ op, count }) => (
                        <span
                          key={op}
                          className="rounded border border-border bg-canvas px-2 py-0.5"
                        >
                          {count} × {t(`triage.ops.${op}`)}
                        </span>
                      ))}
                      <span className="ml-1">· {t('triage.submittedBy', { actor: proposal.submitted_by })}</span>
                      <span>· {formatShortTimestamp(proposal.updated_at)}</span>
                    </div>

                    {proposal.review_note && (
                      <div className="flex items-start gap-1.5 rounded-lg bg-bad-soft/70 px-3 py-2 text-xs text-bad">
                        <MessageSquareWarning className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <span className="line-clamp-2">{proposal.review_note}</span>
                      </div>
                    )}
                  </div>

                  <ChevronRight className="hidden h-5 w-5 shrink-0 text-ink-muted transition-transform group-hover:translate-x-0.5 group-hover:text-accent sm:block" />
                </Link>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
