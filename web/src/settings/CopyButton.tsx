import { Check, Copy } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

type CopyState = 'idle' | 'copied' | 'failed'

interface CopyButtonProps {
  value: string
  /** Accessible name, e.g. "Copy stdio configuration". */
  label: string
  variant?: 'ghost' | 'solid'
}

export function CopyButton({ value, label, variant = 'ghost' }: CopyButtonProps) {
  const { t } = useTranslation()
  const [state, setState] = useState<CopyState>('idle')
  const timer = useRef<number | undefined>(undefined)

  useEffect(() => () => window.clearTimeout(timer.current), [])

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(value)
      setState('copied')
    } catch {
      setState('failed')
    }
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setState('idle'), 2_000)
  }

  const base =
    'inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg px-2.5 text-xs font-medium transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent'
  const tone =
    variant === 'solid'
      ? 'bg-accent text-white hover:bg-accent-hover'
      : 'border border-border bg-surface text-ink-muted hover:text-ink'

  return (
    <button type="button" onClick={() => void handleCopy()} aria-label={label} className={`${base} ${tone}`}>
      {state === 'copied' ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
      <span aria-live="polite">
        {state === 'copied'
          ? t('settings.copy.copied')
          : state === 'failed'
            ? t('settings.copy.failed')
            : t('settings.copy.copy')}
      </span>
    </button>
  )
}
