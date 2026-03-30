/**
 * PersonaSelector — full-screen persona selection for first-time users.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { usePersonaStore } from '@/stores/personaStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { Button } from '@/components/common/Button'
import type { PersonaSummary } from '@/lib/tauri'

interface PersonaSelectorProps {
  onComplete: () => void
}

export function PersonaSelector({ onComplete }: PersonaSelectorProps) {
  const { t } = useTranslation()
  const { personas, setActive } = usePersonaStore()
  const productName = useBrandingStore((s) => s.productName)
  const [selected, setSelected] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)

  const handleConfirm = async () => {
    if (!selected) return
    setConfirming(true)
    try {
      await setActive(selected)
      onComplete()
    } catch (err) {
      console.error('Failed to set active persona:', err)
      setConfirming(false)
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: 'var(--color-bg-base)' }}
    >
      <div className="w-full max-w-4xl px-8">
        <div className="mb-8 text-center">
          <h1
            className="mb-2 text-2xl font-bold"
            style={{ color: 'var(--color-text-primary)' }}
          >
            {t('onboarding.welcome', { productName })}
          </h1>
          <p
            className="text-sm"
            style={{ color: 'var(--color-text-muted)' }}
          >
            {t('onboarding.selectPersona')}
          </p>
        </div>

        <div className="grid grid-cols-4 gap-4">
          {personas.map((p: PersonaSummary) => (
            <button
              key={p.id}
              type="button"
              className="flex flex-col items-center gap-3 rounded-xl p-6 text-center transition-all"
              style={{
                background: selected === p.id ? 'var(--color-bg-elevated)' : 'transparent',
                border: `2px solid ${selected === p.id ? 'var(--color-accent)' : 'var(--color-border-subtle)'}`,
              }}
              onClick={() => setSelected(p.id)}
            >
              <span className="text-4xl">{p.icon}</span>
              <div>
                <div
                  className="mb-1 text-sm font-semibold"
                  style={{ color: 'var(--color-text-primary)' }}
                >
                  {p.name}
                </div>
                <div
                  className="text-xs leading-relaxed"
                  style={{ color: 'var(--color-text-muted)' }}
                >
                  {p.description}
                </div>
              </div>
            </button>
          ))}
        </div>

        <div className="mt-8 flex justify-center">
          <Button
            onClick={handleConfirm}
            disabled={!selected || confirming}
            size="md"
          >
            {confirming ? t('onboarding.settingUp') : t('onboarding.startUsing')}
          </Button>
        </div>

        <p
          className="mt-4 text-center text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {t('onboarding.canSwitchLater')}
        </p>
      </div>
    </div>
  )
}
