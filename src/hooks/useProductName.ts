/**
 * Returns the localized product name.
 * Uses productNameEn when language is 'en-US' and the name is unmodified from DEFAULTS
 * (i.e., not overridden by tenant branding), otherwise returns the configured productName.
 */
import { useTranslation } from 'react-i18next'
import { useBrandingStore, DEFAULTS } from '@/stores/brandingStore'

export function useProductName(): string {
  const { i18n } = useTranslation()
  const productName = useBrandingStore((s) => s.productName)
  const productNameEn = useBrandingStore((s) => s.productNameEn)

  // If tenant has set a custom product name, respect it regardless of language
  if (productName !== DEFAULTS.productName) return productName
  // For the default product name, use the English variant when in English mode
  return i18n.language === 'en-US' ? productNameEn : productName
}
