import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import zh from './locales/zh.json'
import en from './locales/en.json'

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      zh: { translation: zh },
      en: { translation: en },
    },
    supportedLngs: ['zh', 'en'],
    fallbackLng: 'en',
    load: 'languageOnly',
    interpolation: { escapeValue: false }, // React already escapes
    detection: {
      order: ['localStorage', 'navigator'],
      lookupLocalStorage: 'lf_lang',
      caches: ['localStorage'],
    },
    debug: import.meta.env.DEV,
  })

// Keep <html lang> in sync for accessibility / correct browser behavior.
const applyDocLang = (lng: string) => {
  document.documentElement.lang = lng.split('-')[0]
}
applyDocLang(i18n.resolvedLanguage ?? 'en')
i18n.on('languageChanged', applyDocLang)

export default i18n
