import { useTranslation } from 'react-i18next'
import { SUPPORTED_LANGUAGES, type Language } from './i18n'

const LANGUAGE_LABELS: Record<Language, string> = {
  en: 'English',
  'zh-TW': '繁體中文',
}

export default function App() {
  const { t, i18n } = useTranslation()

  return (
    <main className="flex min-h-svh flex-col items-center justify-center gap-6 bg-white px-4 text-slate-900 dark:bg-slate-950 dark:text-slate-100">
      <h1 className="text-4xl font-semibold tracking-tight">Reinforce</h1>
      <p className="max-w-md text-center text-slate-600 dark:text-slate-400">{t('app.tagline')}</p>
      <label className="flex items-center gap-2 text-sm">
        {t('app.language')}
        <select
          className="rounded-md border border-slate-300 bg-transparent px-2 py-1 dark:border-slate-700"
          value={i18n.language}
          onChange={(e) => void i18n.changeLanguage(e.target.value)}
        >
          {SUPPORTED_LANGUAGES.map((lng) => (
            <option key={lng} value={lng}>
              {LANGUAGE_LABELS[lng]}
            </option>
          ))}
        </select>
      </label>
    </main>
  )
}
