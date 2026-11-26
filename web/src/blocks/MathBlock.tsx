import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

type KatexModule = typeof import('katex')

let katexLoader: Promise<KatexModule> | null = null

function loadKatex(): Promise<KatexModule> {
  if (!katexLoader) {
    katexLoader = Promise.all([
      import('katex'),
      import('katex/dist/katex.min.css'),
    ]).then(async ([katexMod]) => {
      // mhchem augments the global katex instance when imported after katex
      await import('katex/contrib/mhchem')
      return katexMod
    })
  }
  return katexLoader
}

export function MathBlock({ latex, display }: { latex: string; display: boolean }) {
  const { t } = useTranslation()
  const [html, setHtml] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    loadKatex()
      .then((mod) => {
        if (cancelled) return
        const katex = mod.default
        const rendered = katex.renderToString(latex, {
          displayMode: display,
          throwOnError: false,
          strict: false,
          trust: false,
        })
        setHtml(rendered)
        setError(null)
      })
      .catch((err: unknown) => {
        if (cancelled) return
        setError(err instanceof Error ? err.message : t('blocks.mathError'))
      })
    return () => {
      cancelled = true
    }
  }, [latex, display, t])

  if (error) {
    return (
      <div className="rounded-lg border border-bad/40 bg-bad-soft p-3 text-sm text-bad">
        <div className="font-medium">{t('blocks.mathError')}</div>
        <pre className="mt-1 overflow-x-auto font-mono text-xs">{latex}</pre>
      </div>
    )
  }

  if (!html) {
    return (
      <div
        className={`${display ? 'my-3 block py-2 text-center' : 'inline-block px-1'} font-mono text-xs text-ink-muted`}
      >
        {latex}
      </div>
    )
  }

  if (display) {
    return (
      <div
        className="my-3 overflow-x-auto rounded-lg border border-border/70 bg-subtle/40 px-4 py-3 text-center"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    )
  }

  return <span className="inline-block align-baseline" dangerouslySetInnerHTML={{ __html: html }} />
}
