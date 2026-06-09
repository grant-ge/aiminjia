import { useTranslation } from 'react-i18next'

import type { AppLanguage } from '@/i18n'
import { useSettingsStore } from '@/stores/settingsStore'

// Endonyms: a language is always shown in its own name regardless of the
// current UI language, so these stay literal rather than going through t().
const LANGUAGE_OPTIONS: Array<{ value: AppLanguage; label: string }> = [
  { value: 'zh-CN', label: '中文' },
  { value: 'en-US', label: 'English' },
]

/**
 * Compact language toggle for the pre-auth login/register screen.
 *
 * Drives selection from the live i18n language (not the settings store, which
 * isn't hydrated from the backend until after sign-in) and switches via the
 * shared `setAppLanguage` action so the choice is persisted to localStorage and
 * kept in lockstep with the in-app Settings → 界面语言 switch.
 */
export function LoginLanguageSwitch() {
  const { t, i18n } = useTranslation()
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)
  const current: AppLanguage = i18n.language === 'en-US' ? 'en-US' : 'zh-CN'

  return (
    <div
      className="inline-flex rounded-md bg-muted p-1"
      role="radiogroup"
      aria-label={t('login.language')}
    >
      {LANGUAGE_OPTIONS.map((option) => {
        const selected = current === option.value
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={option.label}
            onClick={() => setAppLanguage(option.value)}
            className={
              selected
                ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
            }
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}
