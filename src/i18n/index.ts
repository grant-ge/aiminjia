/**
 * i18n initialization — react-i18next with bundled zh-CN and en-US translations.
 * Language preference is stored in localStorage and synced with the settings store.
 */
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import zhCN from './zh-CN.json'
import enUS from './en-US.json'

export type AppLanguage = 'zh-CN' | 'en-US'

const STORAGE_KEY = 'app-language'

function getStoredLanguage(): AppLanguage {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'en-US' || stored === 'zh-CN') return stored
  } catch {
    // ignore localStorage errors
  }
  return 'zh-CN'
}

export function persistLanguage(lang: AppLanguage) {
  try {
    localStorage.setItem(STORAGE_KEY, lang)
  } catch {
    // ignore
  }
}

i18n.use(initReactI18next).init({
  resources: {
    'zh-CN': { translation: zhCN },
    'en-US': { translation: enUS },
  },
  lng: getStoredLanguage(),
  fallbackLng: 'zh-CN',
  interpolation: {
    escapeValue: false, // React already escapes
  },
})

export default i18n
