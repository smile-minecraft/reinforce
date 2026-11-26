import { useTranslation } from 'react-i18next'
import type { SessionMode } from '../api/types'

export function StudyHubPage({ defaultMode }: { defaultMode?: SessionMode }) {
  const { t } = useTranslation()
  return (
    <div className="mx-auto max-w-5xl px-4 py-6">
      <h1 className="text-2xl font-semibold text-ink">{t('study.title')}</h1>
      {defaultMode && <p className="mt-1 text-sm text-ink-muted">{defaultMode}</p>}
    </div>
  )
}
