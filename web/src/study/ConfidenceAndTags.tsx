import { useTranslation } from 'react-i18next'
import type { Confidence } from '../api/types'

const CONFIDENCE_OPTIONS: Confidence[] = ['certain', 'split', 'guessed']

const ERROR_TAG_KEYS = [
  'concept_gap',
  'misread',
  'confused_term',
  'careless',
  'time_pressure',
] as const

export function ConfidenceAndTags({
  confidence,
  onConfidenceChange,
  errorTags,
  onToggleErrorTag,
  confidenceDisabled = false,
  showErrorTags = true,
}: {
  confidence: Confidence
  onConfidenceChange: (value: Confidence) => void
  errorTags: string[]
  onToggleErrorTag: (tag: string) => void
  confidenceDisabled?: boolean
  showErrorTags?: boolean
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3 rounded-xl border border-border bg-surface p-4">
      <div>
        <div className="mb-2 text-xs font-medium text-ink-muted">
          {t('study.runner.confidence')}
        </div>
        <div className="grid grid-cols-3 gap-2">
          {CONFIDENCE_OPTIONS.map((level) => {
            const selected = confidence === level
            return (
              <button
                key={level}
                type="button"
                disabled={confidenceDisabled}
                aria-pressed={selected}
                onClick={() => onConfidenceChange(level)}
                className={`min-h-11 rounded-lg border px-3 text-xs font-medium transition-colors sm:text-sm ${
                  selected
                    ? 'border-accent bg-accent-soft text-accent'
                    : 'border-border bg-canvas text-ink-muted hover:text-ink'
                } disabled:opacity-50`}
              >
                {t(`study.runner.confidenceLevels.${level}`)}
              </button>
            )
          })}
        </div>
      </div>

      {showErrorTags && (
        <div>
          <div className="mb-2 text-xs font-medium text-ink-muted">
            {t('study.runner.errorTags')}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {ERROR_TAG_KEYS.map((tag) => {
              const active = errorTags.includes(tag)
              return (
                <button
                  key={tag}
                  type="button"
                  aria-pressed={active}
                  onClick={() => onToggleErrorTag(tag)}
                  className={`min-h-10 rounded-full border px-3 text-xs font-medium transition-colors ${
                    active
                      ? 'border-warn bg-warn-soft text-warn'
                      : 'border-border bg-canvas text-ink-muted hover:text-ink'
                  }`}
                >
                  {t(`study.runner.errorTagLabels.${tag}`)}
                </button>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
