import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en.json'
import zhTW from './locales/zh-TW.json'

export const SUPPORTED_LANGUAGES = ['en', 'zh-TW'] as const
export type Language = (typeof SUPPORTED_LANGUAGES)[number]

function detectLanguage(): Language {
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-TW' : 'en'
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    'zh-TW': { translation: zhTW },
  },
  lng: detectLanguage(),
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
})

i18n.on('languageChanged', (lng) => {
  document.documentElement.lang = lng
})
document.documentElement.lang = i18n.language

export default i18n
