import { ExternalLink, FileText, ShieldAlert } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { resourceBlobUrl } from '../api/client'
import { useAnchor, useResource } from '../api/hooks/useResources'
import type { Anchor, Resource } from '../api/types'

/** Attribution links come from submitted content, so only plain web URLs become clickable. */
function isHttpUrl(value: string | null | undefined): value is string {
  if (!value) return false
  try {
    const { protocol } = new URL(value)
    return protocol === 'http:' || protocol === 'https:'
  } catch {
    return false
  }
}

function formatMs(ms: number): string {
  const totalSec = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(totalSec / 60)
  const s = totalSec % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

export function AnchorSourceViewer({
  anchorId,
  inlineAnchor,
  inlineResource,
  compact = false,
  showTitle = false,
}: {
  anchorId?: string | null
  inlineAnchor?: Anchor | null
  inlineResource?: Resource | null
  compact?: boolean
  showTitle?: boolean
}) {
  const { t } = useTranslation()
  const {
    data: fetchedAnchor,
    isLoading: loadingAnchor,
    error: anchorError,
  } = useAnchor(inlineAnchor ? undefined : (anchorId ?? undefined))

  const anchor = inlineAnchor ?? fetchedAnchor ?? null

  const {
    data: fetchedResource,
    isLoading: loadingResource,
    error: resourceError,
  } = useResource(inlineResource ? undefined : anchor?.resource_id)

  const resource = inlineResource ?? fetchedResource ?? null

  const loading = (!inlineAnchor && loadingAnchor) || (!inlineResource && loadingResource)
  const error = (!inlineAnchor ? anchorError : null) ?? (!inlineResource ? resourceError : null)
  const blobUrl = resource ? resourceBlobUrl(resource.id) : null
  const pdfUrl =
    blobUrl && anchor?.pdf_page ? `${blobUrl}#page=${anchor.pdf_page}` : blobUrl

  if (loading) {
    return <div className="py-10 text-center text-sm text-ink-muted">{t('common.loading')}</div>
  }

  if (error) {
    return (
      <div className="rounded-lg bg-bad-soft p-3 text-sm text-bad">
        {error instanceof Error ? error.message : t('common.errorGeneric')}
      </div>
    )
  }

  if (!anchor || !resource || !blobUrl) {
    return null
  }

  return (
    <div className="space-y-4">
      {showTitle && (
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-accent" />
          <h3 className="truncate text-sm font-semibold text-ink">{resource.title}</h3>
        </div>
      )}

      {/* Anchor metadata pills */}
      <div className="flex flex-wrap items-center gap-2 text-xs">
        {anchor.pdf_page !== null && (
          <span className="rounded-md bg-accent-soft px-2.5 py-1 font-medium text-accent">
            {t('anchor.pdfPage', { page: anchor.pdf_page })}
          </span>
        )}
        {anchor.printed_page !== null && (
          <span className="rounded-md bg-subtle px-2.5 py-1 font-medium text-ink-muted">
            {t('anchor.printedPage', { page: anchor.printed_page })}
          </span>
        )}
        {anchor.legend_pdf_page !== null && (
          <span className="rounded-md bg-subtle px-2.5 py-1 font-medium text-ink-muted">
            {t('anchor.legendPage', { page: anchor.legend_pdf_page })}
          </span>
        )}
        {anchor.media_span && (
          <span className="rounded-md bg-subtle px-2.5 py-1 font-mono text-ink-muted">
            {t('anchor.mediaSpan', {
              start: formatMs(anchor.media_span.start_ms),
              end: formatMs(anchor.media_span.end_ms),
            })}
          </span>
        )}
        {resource.sensitivity === 'restricted' && (
          <span className="inline-flex items-center gap-1 rounded-md bg-warn-soft px-2.5 py-1 font-medium text-warn">
            <ShieldAlert className="h-3.5 w-3.5" />
            {t('anchor.restrictedNotice', { policy: resource.export_policy })}
          </span>
        )}
      </div>

      {/* Resource Viewer */}
      {resource.kind === 'image' && (
        <div className="relative overflow-hidden rounded-xl border border-border bg-subtle/30">
          <img src={blobUrl} alt={resource.title} className="block h-auto w-full" />
          {anchor.rect && (
            <div
              aria-label={t('anchor.regionHighlight')}
              style={{
                left: `${anchor.rect.x * 100}%`,
                top: `${anchor.rect.y * 100}%`,
                width: `${anchor.rect.w * 100}%`,
                height: `${anchor.rect.h * 100}%`,
              }}
              className="pointer-events-none absolute rounded border-2 border-accent bg-accent/15 shadow-[0_0_0_9999px_rgba(0,0,0,0.22)]"
            />
          )}
        </div>
      )}

      {resource.kind === 'pdf' && pdfUrl && (
        <div className="space-y-3">
          <div className="overflow-hidden rounded-xl border border-border bg-subtle">
            <iframe
              src={pdfUrl}
              title={resource.title}
              className={compact ? 'h-[42dvh] w-full lg:h-[60dvh]' : 'h-[55dvh] w-full'}
            />
          </div>
          <a
            href={pdfUrl}
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-11 items-center gap-2 rounded-lg border border-border bg-canvas px-4 text-sm font-medium text-accent hover:bg-subtle"
          >
            <ExternalLink className="h-4 w-4" />
            <span>{t('anchor.openPdfTab')}</span>
          </a>
        </div>
      )}

      {resource.kind === 'audio' && <audio controls src={blobUrl} className="w-full" />}

      {resource.kind === 'video' && (
        <video controls src={blobUrl} className="w-full rounded-xl border border-border" />
      )}

      {/* Attribution & License */}
      {(resource.license || resource.attribution_notice || resource.attribution_url) && (
        <div className="space-y-1 rounded-lg border border-border bg-subtle/50 p-3 text-xs text-ink-muted">
          {resource.license && (
            <div>
              <span className="font-medium text-ink">{t('anchor.license')}: </span>
              {resource.license}
            </div>
          )}
          {resource.attribution_notice && (
            <div>
              <span className="font-medium text-ink">{t('anchor.attribution')}: </span>
              {resource.attribution_notice}
            </div>
          )}
          {isHttpUrl(resource.attribution_url) && (
            <a
              href={resource.attribution_url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-accent underline"
            >
              {resource.attribution_url}
            </a>
          )}
        </div>
      )}
    </div>
  )
}
