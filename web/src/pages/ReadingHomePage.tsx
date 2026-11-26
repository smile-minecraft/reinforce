import { useTranslation } from 'react-i18next'

export function ReadingHomePage() {
  const { t } = useTranslation()
  return (
    <div className="mx-auto max-w-5xl px-4 py-6">
      <h1 className="text-2xl font-semibold text-ink">{t('reading.title')}</h1>
    </div>
  )
}
