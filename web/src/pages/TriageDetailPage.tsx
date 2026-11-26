import {
  ArrowLeft,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  FileText,
  Lock,
  MessageSquareWarning,
  XCircle,
} from 'lucide-react'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useParams } from 'react-router'
import { ApiError, extractValidationReport } from '../api/client'
import { useAuthStatus } from '../api/hooks/useAuth'
import {
  useApproveProposal,
  useProposal,
  useProposals,
  useRejectProposal,
} from '../api/hooks/useProposals'
import type { ValidationReport } from '../api/types'
import { AnchorSourceViewer } from '../components/AnchorSourceViewer'
import { OperationPreviewCard } from '../triage/OperationPreviewCard'
import { OperationTextEditor } from '../triage/OperationTextEditor'
import { PinCalibrationBlock } from '../triage/PinCalibrationBlock'
import {
  extractProposalAnchors,
  formatShortTimestamp,
  getOperationValidationErrors,
  getProposalOperations,
  getProposalSummaryTitle,
  type ProposalAnchorContext,
} from '../triage/proposalHelpers'
import { RejectModal } from '../triage/RejectModal'
import { ValidationBanner } from '../triage/ValidationBanner'

const SWIPE_COMMIT_PX = 110

function isInteractiveTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tag === 'BUTTON' || tag === 'A') {
    return true
  }
  if (target.isContentEditable) return true
  return Boolean(target.closest('input, textarea, select, button, a, [contenteditable="true"]'))
}

function TriageDetailWorkspace({
  id,
  promoteVerified,
  onPromoteVerifiedChange,
  toast,
  onToast,
}: {
  id: string | undefined
  promoteVerified: boolean
  onPromoteVerifiedChange: (next: boolean) => void
  toast: string | null
  onToast: (message: string) => void
}) {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const { data: authStatus } = useAuthStatus()
  const isNonOwner = Boolean(authStatus?.role && authStatus.role !== 'owner')

  const { data: proposal, isLoading, error, refetch } = useProposal(id)
  const { data: pendingList = [] } = useProposals('pending')

  const approveMutation = useApproveProposal()
  const rejectMutation = useRejectProposal()

  const [selectedAnchorId, setSelectedAnchorId] = useState<string | null>(null)
  const [mobileSourceOpen, setMobileSourceOpen] = useState(true)
  const [rejectOpen, setRejectOpen] = useState(false)
  const [validationReport, setValidationReport] = useState<ValidationReport | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [editingOpIndex, setEditingOpIndex] = useState<number | null>(null)

  const [swipeDx, setSwipeDx] = useState(0)
  const swipeRef = useRef<{
    pointerId: number
    startX: number
    startY: number
    locked: 'none' | 'horizontal' | 'vertical'
  } | null>(null)

  const operations = useMemo(
    () => (proposal ? getProposalOperations(proposal) : []),
    [proposal],
  )
  const anchorCtx = useMemo<ProposalAnchorContext>(
    () =>
      proposal
        ? extractProposalAnchors(proposal)
        : { anchorIds: [], stagedAnchors: new Map(), stagedResources: new Map() },
    [proposal],
  )

  const activeAnchorId =
    selectedAnchorId && anchorCtx.anchorIds.includes(selectedAnchorId)
      ? selectedAnchorId
      : (anchorCtx.anchorIds[0] ?? null)

  const pendingIndex = useMemo(
    () => pendingList.findIndex((p) => p.id === id),
    [pendingList, id],
  )

  const prevPendingId = pendingIndex > 0 ? pendingList[pendingIndex - 1].id : null
  const nextPendingId = useMemo(() => {
    if (pendingList.length === 0) return null
    if (pendingIndex >= 0) {
      if (pendingIndex + 1 < pendingList.length) return pendingList[pendingIndex + 1].id
      if (pendingIndex - 1 >= 0) return pendingList[pendingIndex - 1].id
      return null
    }
    const other = pendingList.find((p) => p.id !== id)
    return other?.id ?? null
  }, [pendingList, pendingIndex, id])

  const isPendingProposal = proposal?.status === 'pending'
  const isBusy = approveMutation.isPending || rejectMutation.isPending

  const advanceAfterDecision = useCallback(
    (message: string) => {
      if (nextPendingId) {
        onToast(message)
        void navigate(`/triage/${encodeURIComponent(nextPendingId)}`, { replace: true })
      } else {
        void navigate('/triage', {
          replace: true,
          state: { notice: t('triage.allCaughtUp') },
        })
      }
    },
    [navigate, nextPendingId, onToast, t],
  )

  const handleApprove = useCallback(async () => {
    if (!proposal || !isPendingProposal || isBusy) return
    setActionError(null)
    setValidationReport(null)
    try {
      await approveMutation.mutateAsync({
        id: proposal.id,
        promoteVerified,
      })
      advanceAfterDecision(t('triage.approvedToast'))
    } catch (err) {
      const report = extractValidationReport(err)
      if (report) {
        setValidationReport(report)
      } else {
        setActionError(err instanceof Error ? err.message : t('common.errorGeneric'))
      }
    }
  }, [advanceAfterDecision, approveMutation, isBusy, isPendingProposal, promoteVerified, proposal, t])

  const handleRejectConfirm = useCallback(
    async (reviewNote: string) => {
      if (!proposal || !isPendingProposal || isBusy) return
      setActionError(null)
      try {
        await rejectMutation.mutateAsync({
          id: proposal.id,
          reviewNote,
        })
        setRejectOpen(false)
        advanceAfterDecision(t('triage.rejectedToast'))
      } catch (err) {
        setActionError(err instanceof Error ? err.message : t('common.errorGeneric'))
      }
    },
    [advanceAfterDecision, isBusy, isPendingProposal, proposal, rejectMutation, t],
  )

  // Spacebar quick-approve shortcut (ignored while typing, editing, or in modal)
  useEffect(() => {
    if (!isPendingProposal || rejectOpen || editingOpIndex !== null) return
    function onKeyDown(e: KeyboardEvent) {
      if (e.code !== 'Space' && e.key !== ' ') return
      if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return
      if (isInteractiveTarget(e.target)) return
      e.preventDefault()
      void handleApprove()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [editingOpIndex, handleApprove, isPendingProposal, rejectOpen])

  function handleSwipePointerDown(e: ReactPointerEvent<HTMLDivElement>) {
    if (!isPendingProposal || isBusy || rejectOpen || editingOpIndex !== null) return
    const target = e.target as HTMLElement | null
    if (
      target?.closest(
        '[data-no-swipe="true"], .rf-table-wrap, input, textarea, select, button, a',
      )
    ) {
      return
    }
    swipeRef.current = {
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      locked: 'none',
    }
  }

  function handleSwipePointerMove(e: ReactPointerEvent<HTMLDivElement>) {
    const st = swipeRef.current
    if (!st || st.pointerId !== e.pointerId) return
    const dx = e.clientX - st.startX
    const dy = e.clientY - st.startY
    if (st.locked === 'none') {
      if (Math.abs(dx) > 22 && Math.abs(dx) > Math.abs(dy) * 1.5) {
        st.locked = 'horizontal'
      } else if (Math.abs(dy) > 16) {
        st.locked = 'vertical'
        return
      } else {
        return
      }
    }
    if (st.locked === 'horizontal') {
      setSwipeDx(Math.max(-160, Math.min(160, dx)))
    }
  }

  function handleSwipePointerEnd(e: ReactPointerEvent<HTMLDivElement>) {
    const st = swipeRef.current
    if (!st || st.pointerId !== e.pointerId) return
    const dx = st.locked === 'horizontal' ? e.clientX - st.startX : 0
    swipeRef.current = null
    setSwipeDx(0)
    if (dx >= SWIPE_COMMIT_PX) {
      void handleApprove()
    } else if (dx <= -SWIPE_COMMIT_PX) {
      setRejectOpen(true)
    }
  }

  function jumpToOperation(opIndex: number) {
    const el = document.getElementById(`triage-op-${opIndex}`)
    el?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }

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

  if (isLoading) {
    return <div className="py-16 text-center text-sm text-ink-muted">{t('common.loading')}</div>
  }

  if (error || !proposal) {
    return (
      <div className="mx-auto max-w-3xl space-y-4 px-4 py-10 sm:px-6">
        <Link
          to="/triage"
          className="inline-flex min-h-11 items-center gap-1.5 text-sm font-medium text-accent hover:underline"
        >
          <ArrowLeft className="h-4 w-4" />
          <span>{t('common.back')}</span>
        </Link>
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-bad/40 bg-bad-soft p-4 text-sm text-bad">
          <span>{error instanceof Error ? error.message : t('common.errorGeneric')}</span>
          <button
            type="button"
            onClick={() => void refetch()}
            className="min-h-10 rounded-lg border border-bad/40 bg-surface px-3 text-xs font-medium text-bad"
          >
            {t('common.retry')}
          </button>
        </div>
      </div>
    )
  }

  const activeAnchor = activeAnchorId
    ? anchorCtx.stagedAnchors.get(activeAnchorId) ?? null
    : null
  const activeInlineResource = activeAnchor
    ? anchorCtx.stagedResources.get(activeAnchor.resource_id) ?? null
    : null

  return (
    <div className="mx-auto max-w-7xl space-y-4 px-4 py-4 sm:px-6 lg:py-6">
      {/* Top workbench bar */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-surface px-4 py-3">
        <div className="flex min-w-0 items-center gap-3">
          <Link
            to="/triage"
            className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-lg border border-border bg-canvas text-ink-muted hover:bg-subtle hover:text-ink"
            aria-label={t('common.back')}
          >
            <ArrowLeft className="h-4 w-4" />
          </Link>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="truncate text-base font-semibold text-ink">
                {getProposalSummaryTitle(proposal)}
              </h1>
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
            </div>
            <div className="flex flex-wrap items-center gap-2 text-xs text-ink-muted">
              <span>{t('triage.submittedBy', { actor: proposal.submitted_by })}</span>
              <span>· {formatShortTimestamp(proposal.updated_at)}</span>
              {pendingIndex >= 0 && (
                <span>
                  · {t('triage.queueProgress', { current: pendingIndex + 1, total: pendingList.length })}
                </span>
              )}
            </div>
          </div>
        </div>

        {/* Queue prev / next */}
        {pendingList.length > 1 && (
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              disabled={!prevPendingId}
              onClick={() =>
                prevPendingId && void navigate(`/triage/${encodeURIComponent(prevPendingId)}`)
              }
              aria-label={t('triage.prevProposal')}
              className="flex min-h-11 min-w-11 items-center justify-center rounded-lg border border-border bg-canvas text-ink hover:bg-subtle disabled:opacity-40"
            >
              <ChevronLeft className="h-4 w-4" />
            </button>
            <button
              type="button"
              disabled={!nextPendingId}
              onClick={() =>
                nextPendingId && void navigate(`/triage/${encodeURIComponent(nextPendingId)}`)
              }
              aria-label={t('triage.nextProposal')}
              className="flex min-h-11 min-w-11 items-center justify-center rounded-lg border border-border bg-canvas text-ink hover:bg-subtle disabled:opacity-40"
            >
              <ChevronRight className="h-4 w-4" />
            </button>
          </div>
        )}
      </div>

      {toast && (
        <div className="flex items-center gap-2 rounded-xl border border-ok/40 bg-ok-soft px-4 py-2.5 text-sm font-medium text-ok">
          <CheckCircle2 className="h-4 w-4 shrink-0" />
          <span>{toast}</span>
        </div>
      )}

      {actionError && !rejectOpen && (
        <div role="alert" className="rounded-xl border border-bad/40 bg-bad-soft px-4 py-3 text-sm text-bad">
          {actionError}
        </div>
      )}

      <ValidationBanner report={validationReport} onSelectOperation={jumpToOperation} />

      {/* Split-pane Workbench: Stacked on mobile, Side-by-side on lg+ */}
      <div className="grid grid-cols-1 gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)] lg:items-start">
        {/* LEFT / TOP: Source Anchor Pane */}
        <section className="rounded-xl border border-border bg-surface lg:sticky lg:top-6 lg:flex lg:max-h-[calc(100dvh-3rem)] lg:flex-col">
          <div className="flex items-center justify-between border-b border-border px-4 py-3">
            <div className="flex items-center gap-2 text-sm font-semibold text-ink">
              <FileText className="h-4 w-4 text-accent" />
              <h2>{t('triage.sourcePane')}</h2>
              {anchorCtx.anchorIds.length > 0 && (
                <span className="rounded bg-subtle px-1.5 py-0.5 font-mono text-xs text-ink-muted">
                  {anchorCtx.anchorIds.length}
                </span>
              )}
            </div>

            <button
              type="button"
              onClick={() => setMobileSourceOpen((v) => !v)}
              className="inline-flex min-h-10 items-center gap-1 rounded-lg border border-border px-2.5 text-xs font-medium text-ink-muted hover:bg-subtle lg:hidden"
            >
              <span>{t('triage.toggleSourcePane')}</span>
              {mobileSourceOpen ? (
                <ChevronUp className="h-4 w-4" />
              ) : (
                <ChevronDown className="h-4 w-4" />
              )}
            </button>
          </div>

          <div
            className={`${mobileSourceOpen ? 'block' : 'hidden'} overflow-y-auto p-4 lg:block`}
          >
            {anchorCtx.anchorIds.length === 0 ? (
              <div className="rounded-lg border border-dashed border-border bg-subtle/30 p-8 text-center text-sm text-ink-muted">
                {t('triage.noAnchorInProposal')}
              </div>
            ) : (
              <div className="space-y-3">
                {anchorCtx.anchorIds.length > 1 && (
                  <div className="flex flex-wrap items-center gap-1.5">
                    {anchorCtx.anchorIds.map((aid, idx) => (
                      <button
                        key={aid}
                        type="button"
                        onClick={() => setSelectedAnchorId(aid)}
                        className={`min-h-10 rounded-lg border px-3 text-xs font-medium transition-colors ${
                          activeAnchorId === aid
                            ? 'border-accent bg-accent-soft text-accent'
                            : 'border-border bg-canvas text-ink-muted hover:bg-subtle hover:text-ink'
                        }`}
                      >
                        {t('triage.sourceAnchorIndex', { index: idx + 1 })}
                      </button>
                    ))}
                  </div>
                )}

                <AnchorSourceViewer
                  anchorId={activeAnchorId}
                  inlineAnchor={activeAnchor}
                  inlineResource={activeInlineResource}
                  compact
                  showTitle
                />
              </div>
            )}
          </div>
        </section>

        {/* RIGHT / BOTTOM: Staged Draft & Review Actions */}
        <section className="space-y-4">
          {!isPendingProposal && (
            <div className="rounded-xl border border-border bg-subtle/60 p-4 text-sm text-ink-muted">
              <div>
                {t('triage.alreadyReviewed', { status: t(`triage.status.${proposal.status}`) })}
              </div>
              {proposal.review_note && (
                <div className="mt-2 flex items-start gap-2 rounded-lg bg-bad-soft p-3 text-xs text-bad">
                  <MessageSquareWarning className="mt-0.5 h-4 w-4 shrink-0" />
                  <div>
                    <span className="font-semibold">{t('triage.reviewNote')}: </span>
                    {proposal.review_note}
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Swipeable draft container */}
          <div
            onPointerDown={handleSwipePointerDown}
            onPointerMove={handleSwipePointerMove}
            onPointerUp={handleSwipePointerEnd}
            onPointerCancel={handleSwipePointerEnd}
            className="relative space-y-4"
            style={{
              transform: swipeDx !== 0 ? `translate3d(${swipeDx}px, 0, 0)` : undefined,
              transition: swipeDx === 0 ? 'transform 180ms ease-out' : undefined,
            }}
          >
            {swipeDx >= 40 && (
              <div className="pointer-events-none sticky top-4 z-20 flex items-center justify-center rounded-xl border-2 border-ok bg-ok-soft px-4 py-2 text-sm font-semibold text-ok shadow-md">
                <Check className="mr-1.5 h-4 w-4" />
                {t('triage.swipeApproveHint')}
              </div>
            )}
            {swipeDx <= -40 && (
              <div className="pointer-events-none sticky top-4 z-20 flex items-center justify-center rounded-xl border-2 border-bad bg-bad-soft px-4 py-2 text-sm font-semibold text-bad shadow-md">
                <XCircle className="mr-1.5 h-4 w-4" />
                {t('triage.swipeRejectHint')}
              </div>
            )}

            {operations.map((op, index) => {
              const opErrors = getOperationValidationErrors(validationReport, index, op.temp_ref)
              return (
                <OperationPreviewCard
                  key={`${op.op}-${op.id ?? index}`}
                  op={op}
                  index={index}
                  opErrors={opErrors}
                  canEdit={isPendingProposal}
                  isEditing={editingOpIndex === index}
                  onToggleEdit={() =>
                    setEditingOpIndex((prev) => (prev === index ? null : index))
                  }
                  onSelectAnchor={(aid) => {
                    setSelectedAnchorId(aid)
                    setMobileSourceOpen(true)
                  }}
                  renderInteractivePins={(block) => (
                    <PinCalibrationBlock
                      proposalId={proposal.id}
                      resourceId={block.resource_id}
                      overlayResourceId={block.overlay_resource_id}
                      pins={block.pins}
                      readOnly={!isPendingProposal}
                      onValidationError={(report) => setValidationReport(report)}
                    />
                  )}
                  editorNode={
                    <OperationTextEditor
                      proposalId={proposal.id}
                      op={op}
                      opIndex={index}
                      opErrors={opErrors}
                      onSaved={() => {
                        setEditingOpIndex(null)
                        setValidationReport(null)
                        onToast(t('triage.saved'))
                      }}
                      onCancel={() => setEditingOpIndex(null)}
                      onValidationError={(report) => setValidationReport(report)}
                    />
                  }
                />
              )
            })}
          </div>

          {/* Sticky Decision Bar */}
          {isPendingProposal && (
            <div
              data-no-swipe="true"
              className="sticky bottom-16 z-20 rounded-xl border border-border bg-surface/95 p-4 shadow-lg backdrop-blur lg:bottom-4"
            >
              <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <label className="inline-flex min-h-11 cursor-pointer items-center gap-2.5 text-sm text-ink select-none">
                  <input
                    type="checkbox"
                    checked={promoteVerified}
                    onChange={(e) => onPromoteVerifiedChange(e.target.checked)}
                    className="h-5 w-5 rounded border-border accent-accent"
                  />
                  <span>{t('triage.promoteVerified')}</span>
                </label>

                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => {
                      setActionError(null)
                      setRejectOpen(true)
                    }}
                    className="inline-flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg border border-bad/40 bg-bad-soft px-4 text-sm font-medium text-bad hover:bg-bad hover:text-white disabled:opacity-45 sm:flex-initial"
                  >
                    <XCircle className="h-4 w-4" />
                    <span>{t('triage.reject')}</span>
                  </button>

                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => void handleApprove()}
                    className="inline-flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg bg-ok px-5 text-sm font-semibold text-white hover:opacity-90 disabled:opacity-45 sm:flex-initial"
                  >
                    <CheckCircle2 className="h-4 w-4" />
                    <span>
                      {approveMutation.isPending ? t('triage.approving') : t('triage.approve')}
                    </span>
                    <kbd className="hidden rounded bg-white/20 px-1.5 py-0.5 font-mono text-[11px] sm:inline">
                      {t('triage.approveShortcut')}
                    </kbd>
                  </button>
                </div>
              </div>
              <p className="mt-2 text-center text-xs text-ink-muted sm:text-left">
                {t('triage.swipeHint')}
              </p>
            </div>
          )}
        </section>
      </div>

      <RejectModal
        open={rejectOpen}
        isPending={rejectMutation.isPending}
        errorMessage={actionError}
        onClose={() => setRejectOpen(false)}
        onConfirm={(note) => void handleRejectConfirm(note)}
      />
    </div>
  )
}

export function TriageDetailPage() {
  const { id } = useParams<{ id: string }>()
  const [promoteVerified, setPromoteVerified] = useState(true)
  const [toast, setToast] = useState<string | null>(null)

  useEffect(() => {
    if (!toast) return
    const timer = window.setTimeout(() => setToast(null), 2600)
    return () => window.clearTimeout(timer)
  }, [toast])

  return (
    <TriageDetailWorkspace
      key={id ?? 'missing'}
      id={id}
      promoteVerified={promoteVerified}
      onPromoteVerifiedChange={setPromoteVerified}
      toast={toast}
      onToast={setToast}
    />
  )
}
