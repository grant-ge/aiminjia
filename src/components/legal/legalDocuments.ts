import privacyHtml from '@/assets/legal/ai-xiaojia-privacy.html?raw'
import termsHtml from '@/assets/legal/ai-xiaojia-terms.html?raw'

export type LegalDocumentKey = 'terms' | 'privacy'

export interface LegalDocument {
  key: LegalDocumentKey
  title: string
  html: string
}

export const LEGAL_DOCUMENTS: Record<LegalDocumentKey, LegalDocument> = {
  terms: {
    key: 'terms',
    title: 'AI小家软件许可及服务协议',
    html: termsHtml,
  },
  privacy: {
    key: 'privacy',
    title: 'AI小家隐私政策',
    html: privacyHtml,
  },
}
