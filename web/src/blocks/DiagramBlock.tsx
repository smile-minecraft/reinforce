import DOMPurify from 'dompurify'
import { useEffect, useId, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import type { DiagramNodeBinding, DiagramSyntax } from '../api/types'
import { useTheme } from '../components/themeContext'

function sanitizeSvg(svg: string): string {
  return DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ['foreignObject'],
  })
}

export function DiagramBlock({
  source,
  syntax,
  svgCache,
  nodeBindings,
}: {
  source: string
  syntax: DiagramSyntax
  svgCache: string | null
  nodeBindings: DiagramNodeBinding[]
}) {
  const { t } = useTranslation()
  const { resolved } = useTheme()
  const uniqueId = useId().replace(/[^a-zA-Z0-9_-]/g, '')
  const [renderedSvg, setRenderedSvg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const precompiledSvg = useMemo(() => {
    if (svgCache) return sanitizeSvg(svgCache)
    if (syntax === 'svg') return sanitizeSvg(source)
    return null
  }, [svgCache, syntax, source])

  useEffect(() => {
    if (precompiledSvg || syntax !== 'mermaid') {
      setRenderedSvg(null)
      setError(null)
      setLoading(false)
      return
    }

    let cancelled = false
    setLoading(true)
    setError(null)

    import('mermaid')
      .then(async (mod) => {
        if (cancelled) return
        const mermaid = mod.default
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: 'strict',
          theme: resolved === 'dark' ? 'dark' : 'neutral',
          fontFamily: 'Inter, Noto Sans TC, system-ui, sans-serif',
        })
        const renderId = `rf-mermaid-${uniqueId}-${resolved}`
        const { svg } = await mermaid.render(renderId, source)
        if (!cancelled) {
          setRenderedSvg(sanitizeSvg(svg))
          setLoading(false)
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : t('blocks.diagramError'))
          setLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [precompiledSvg, syntax, source, resolved, uniqueId, t])

  const finalSvg = precompiledSvg ?? renderedSvg

  return (
    <div className="my-3 rounded-xl border border-border bg-surface p-3 sm:p-4">
      {finalSvg ? (
        <div
          className="rf-diagram overflow-x-auto text-ink"
          dangerouslySetInnerHTML={{ __html: finalSvg }}
        />
      ) : loading ? (
        <div className="py-6 text-center text-sm text-ink-muted">{t('blocks.diagramLoading')}</div>
      ) : error ? (
        <div className="rounded-lg bg-bad-soft p-3 text-sm text-bad">
          <div className="font-medium">{t('blocks.diagramError')}</div>
          <pre className="mt-1.5 overflow-x-auto font-mono text-xs text-ink-muted">{source}</pre>
        </div>
      ) : (
        <div>
          <div className="mb-1.5 text-xs font-medium text-ink-muted">
            {t('blocks.diagramSource')} ({syntax})
          </div>
          <pre className="overflow-x-auto rounded-lg bg-subtle p-3 font-mono text-xs text-ink">
            {source}
          </pre>
        </div>
      )}

      {nodeBindings.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-2 border-t border-border pt-3">
          {nodeBindings.map((binding) => (
            <Link
              key={`${binding.node_id}-${binding.card_id}`}
              to={`/cards/${encodeURIComponent(binding.card_id)}`}
              className="inline-flex min-h-9 items-center rounded-md border border-border bg-subtle px-2.5 text-xs font-medium text-accent hover:bg-accent-soft"
            >
              {binding.node_id}
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
