import { X, XCircle } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

function RejectModalContent({
  isPending,
  errorMessage,
  onClose,
  onConfirm,
}: {
  isPending: boolean
  errorMessage: string | null
  onClose: () => void
  onConfirm: (reviewNote: string) => void
}) {
  const { t } = useTranslation()
  const [note, setNote] = useState('')
  const [touched, setTouched] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)

  useEffect(() => {
    const timer = window.setTimeout(() => textareaRef.current?.focus(), 40)
    return () => window.clearTimeout(timer)
  }, [])

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape' && !isPending) onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [isPending, onClose])

  const quickChips = [
    t('triage.rejectQuickChips.pinOffset'),
    t('triage.rejectQuickChips.missingBasis'),
    t('triage.rejectQuickChips.mismatchSource'),
  ]

  function appendChip(text: string) {
    setNote((prev) => (prev.trim() ? `${prev.trim()}\n${text}` : text))
    textareaRef.current?.focus()
  }

  function handleSubmit() {
    setTouched(true)
    const trimmed = note.trim()
    if (!trimmed) return
    onConfirm(trimmed)
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center sm:items-center sm:p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t('triage.rejectTitle')}
      data-no-swipe="true"
    >
      <button
        type="button"
        aria-label={t('common.close')}
        disabled={isPending}
        onClick={onClose}
        className="fixed inset-0 bg-black/50 backdrop-blur-[1px]"
      />

      <div className="relative z-10 w-full max-w-lg rounded-t-2xl border border-border bg-surface p-5 shadow-2xl sm:rounded-2xl">
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-center gap-2 text-base font-semibold text-ink">
            <XCircle className="h-5 w-5 text-bad" />
            <h2>{t('triage.rejectTitle')}</h2>
          </div>
          <button
            type="button"
            disabled={isPending}
            onClick={onClose}
            aria-label={t('common.close')}
            className="flex min-h-11 min-w-11 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <p className="mt-1 text-sm text-ink-muted">{t('triage.rejectDesc')}</p>

        {/* Quick feedback chips */}
        <div className="mt-3 flex flex-wrap gap-1.5">
          {quickChips.map((chip) => (
            <button
              key={chip}
              type="button"
              onClick={() => appendChip(chip)}
              className="min-h-9 rounded-lg border border-border bg-subtle/60 px-2.5 py-1 text-left text-xs text-ink-muted hover:border-accent hover:text-ink"
            >
              + {chip}
            </button>
          ))}
        </div>

        <div className="mt-3">
          <label htmlFor="reject-review-note" className="block text-xs font-medium text-ink">
            {t('triage.rejectNoteLabel')}
          </label>
          <textarea
            id="reject-review-note"
            ref={textareaRef}
            rows={4}
            value={note}
            onChange={(e) => setNote(e.target.value)}
            placeholder={t('triage.rejectNotePlaceholder')}
            className="mt-1.5 w-full rounded-lg border border-border bg-canvas p-3 text-sm text-ink placeholder:text-ink-muted"
          />
          {touched && !note.trim() && (
            <p className="mt-1 text-xs text-bad">{t('triage.rejectNoteRequired')}</p>
          )}
        </div>

        {errorMessage && (
          <div className="mt-3 rounded-lg bg-bad-soft px-3 py-2 text-xs text-bad">
            {errorMessage}
          </div>
        )}

        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            disabled={isPending}
            onClick={onClose}
            className="min-h-11 rounded-lg border border-border bg-canvas px-4 text-sm font-medium text-ink hover:bg-subtle"
          >
            {t('common.cancel')}
          </button>
          <button
            type="button"
            disabled={isPending || !note.trim()}
            onClick={handleSubmit}
            className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-bad px-4 text-sm font-medium text-white hover:opacity-90 disabled:opacity-45"
          >
            <XCircle className="h-4 w-4" />
            <span>{isPending ? t('triage.rejecting') : t('triage.confirmReject')}</span>
          </button>
        </div>
      </div>
    </div>
  )
}

export function RejectModal({
  open,
  isPending,
  errorMessage,
  onClose,
  onConfirm,
}: {
  open: boolean
  isPending: boolean
  errorMessage: string | null
  onClose: () => void
  onConfirm: (reviewNote: string) => void
}) {
  if (!open) return null
  return (
    <RejectModalContent
      isPending={isPending}
      errorMessage={errorMessage}
      onClose={onClose}
      onConfirm={onConfirm}
    />
  )
}
