import privacyHtmlZh from '@/assets/legal/ai-xiaojia-privacy.html?raw'
import privacyHtmlEn from '@/assets/legal/ai-xiaojia-privacy.en-US.html?raw'
import termsHtmlZh from '@/assets/legal/ai-xiaojia-terms.html?raw'
import termsHtmlEn from '@/assets/legal/ai-xiaojia-terms.en-US.html?raw'

export type LegalDocumentKey = 'terms' | 'privacy'

export interface LegalDocument {
  key: LegalDocumentKey
  /** i18n key resolved by the caller (e.g. `legal.terms.title`). */
  titleKey: string
  html: string
}

const HTML: Record<LegalDocumentKey, { 'zh-CN': string; 'en-US': string }> = {
  terms: { 'zh-CN': termsHtmlZh, 'en-US': termsHtmlEn },
  privacy: { 'zh-CN': privacyHtmlZh, 'en-US': privacyHtmlEn },
}

const TITLE_KEYS: Record<LegalDocumentKey, string> = {
  terms: 'legal.terms.title',
  privacy: 'legal.privacy.title',
}

export function getLegalDocument(key: LegalDocumentKey, language: string): LegalDocument {
  const variants = HTML[key]
  const html = language?.startsWith('en') ? variants['en-US'] : variants['zh-CN']
  return { key, titleKey: TITLE_KEYS[key], html }
}
