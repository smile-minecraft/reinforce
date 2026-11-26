import { Eye, EyeOff } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { resourceBlobUrl } from '../api/client'
import type { OcclusionMask } from '../api/types'

export function ImageOcclusionBlock({
  resourceId,
  masks,
}: {
  resourceId: string
  masks: OcclusionMask[]
}) {
  const { t } = useTranslation()
  const [revealedGroups, setRevealedGroups] = useState<Set<string>>(() => new Set())

  const allGroups = Array.from(new Set(masks.map((m) => m.group_id)))
  const allRevealed = allGroups.length > 0 && allGroups.every((g) => revealedGroups.has(g))

  function toggleGroup(groupId: string) {
    setRevealedGroups((prev) => {
      const next = new Set(prev)
      if (next.has(groupId)) next.delete(groupId)
      else next.add(groupId)
      return next
    })
  }

  function revealAll() {
    setRevealedGroups(new Set(allGroups))
  }

  function hideAll() {
    setRevealedGroups(new Set())
  }

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-border bg-surface">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-subtle/60 px-3 py-2">
        <span className="text-xs text-ink-muted">{t('blocks.occlusionHint')}</span>
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={allRevealed ? hideAll : revealAll}
            className="inline-flex min-h-11 items-center gap-1.5 rounded-lg border border-border bg-surface px-3 text-xs font-medium text-ink hover:bg-subtle"
          >
            {allRevealed ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
            <span>{allRevealed ? t('blocks.hideAll') : t('blocks.revealAll')}</span>
          </button>
        </div>
      </div>

      <div className="relative select-none bg-subtle/30">
        <img
          src={resourceBlobUrl(resourceId)}
          alt=""
          className="block h-auto w-full"
          draggable={false}
        />
        {masks.map((mask, index) => {
          const isRevealed = revealedGroups.has(mask.group_id)
          return (
            <button
              key={`${mask.group_id}-${index}`}
              type="button"
              onClick={() => toggleGroup(mask.group_id)}
              aria-pressed={isRevealed}
              aria-label={t('blocks.maskGroup', { group: mask.group_id })}
              style={{
                left: `${mask.rect.x * 100}%`,
                top: `${mask.rect.y * 100}%`,
                width: `${mask.rect.w * 100}%`,
                height: `${mask.rect.h * 100}%`,
              }}
              className={`absolute flex items-center justify-center rounded border-2 text-xs font-semibold transition-all ${
                isRevealed
                  ? 'border-accent/60 bg-accent/10 text-transparent hover:bg-accent/20'
                  : 'border-accent bg-accent text-white shadow-sm hover:bg-accent-hover'
              }`}
            >
              {!isRevealed && <span className="truncate px-1">{mask.group_id}</span>}
            </button>
          )
        })}
      </div>
    </div>
  )
}
