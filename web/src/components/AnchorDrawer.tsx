import { FileText, X } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useAnchor, useResource } from '../api/hooks/useResources'
import { AnchorSourceViewer } from './AnchorSourceViewer'

export function AnchorDrawer({
  anchorId,
  onClose,
}: {
  anchorId: string | null
  onClose: () => void
}) {
  const { t } = useTranslation()
  const { data: anchor } = useAnchor(anchorId ?? undefined)
  const { data: resource } = useResource(anchor?.resource_id)

  useEffect(() => {
    if (!anchorId) return
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [anchorId, onClose])

  if (!anchorId) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-end md:items-stretch"
      role="dialog"
      aria-modal="true"
      aria-label={t('anchor.title')}
    >
      {/* Backdrop */}
      <button
        type="button"
        aria-label={t('common.close')}
        onClick={onClose}
        className="fixed inset-0 bg-black/45 backdrop-blur-[1px]"
      />

      {/* Drawer panel: bottom sheet on mobile, right slide-over on md+ */}
      <div className="relative z-10 flex max-h-[85dvh] w-full flex-col rounded-t-2xl border-t border-border bg-surface shadow-2xl md:h-full md:max-h-none md:max-w-xl md:rounded-t-none md:border-t-0 md:border-l">
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex min-w-0 items-center gap-2">
            <FileText className="h-4 w-4 shrink-0 text-accent" />
            <h2 className="truncate text-sm font-semibold text-ink">
              {resource?.title ?? t('anchor.title')}
            </h2>
          </div>

          <button
            type="button"
            onClick={onClose}
            aria-label={t('common.close')}
            className="flex min-h-11 min-w-11 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4">
          <AnchorSourceViewer anchorId={anchorId} />
        </div>
      </div>
    </div>
  )
}
