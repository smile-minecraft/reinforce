import { AlertTriangle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { ValidationErrorItem, ValidationReport } from '../api/types'

export function ValidationBanner({
  report,
  onSelectOperation,
}: {
  report: ValidationReport | null
  onSelectOperation?: (opIndex: number) => void
}) {
  const { t } = useTranslation()

  if (!report || report.errors.length === 0) return null

  return (
    <div
      role="alert"
      className="rounded-xl border border-bad/40 bg-bad-soft p-4 text-sm text-bad"
    >
      <div className="flex items-center gap-2 font-semibold">
        <AlertTriangle className="h-4 w-4 shrink-0" />
        <span>{t('triage.validationHeading', { count: report.errors.length })}</span>
      </div>

      <ul className="mt-2.5 space-y-1.5">
        {report.errors.map((err, idx) => {
          const canJump = err.op_index !== null && Boolean(onSelectOperation)
          return (
            <li
              key={`${err.op_index ?? 'global'}-${err.field_path}-${idx}`}
              className="flex flex-wrap items-baseline gap-1.5 text-xs"
            >
              {err.op_index !== null && (
                <button
                  type="button"
                  disabled={!canJump}
                  onClick={() => err.op_index !== null && onSelectOperation?.(err.op_index)}
                  className="rounded bg-bad/15 px-1.5 py-0.5 font-mono font-semibold text-bad hover:bg-bad/25 disabled:cursor-default"
                >
                  {t('triage.validationOpRef', { index: err.op_index + 1 })}
                </button>
              )}
              <code className="rounded bg-surface/80 px-1.5 py-0.5 font-mono text-[11px] text-ink">
                {err.field_path}
              </code>
              <span className="rounded bg-bad/10 px-1.5 py-0.5 font-mono text-[11px] opacity-85">
                {err.rule}
              </span>
              <span className="text-ink">{err.message}</span>
            </li>
          )
        })}
      </ul>
    </div>
  )
}

export function FieldErrorList({ errors }: { errors: ValidationErrorItem[] }) {
  if (errors.length === 0) return null

  return (
    <div className="mt-1.5 space-y-1">
      {errors.map((err, idx) => (
        <div
          key={`${err.field_path}-${idx}`}
          className="flex items-start gap-1.5 rounded-md border border-bad/30 bg-bad-soft px-2.5 py-1.5 text-xs text-bad"
        >
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <div>
            <span className="mr-1.5 font-mono font-semibold">{err.field_path}</span>
            <span>{err.message}</span>
          </div>
        </div>
      ))}
    </div>
  )
}
