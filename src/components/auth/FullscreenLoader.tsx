import { useTranslation } from 'react-i18next'

import { Spinner } from '@/components/ui/spinner'

export function FullscreenLoader() {
  const { t } = useTranslation()
  return (
    <div
      data-testid="fullscreen-loader"
      className="fixed inset-0 flex items-center justify-center"
      style={{
        animation: 'fadeInDelayed 0.2s ease forwards',
        animationDelay: '300ms',
        background: 'var(--color-bg-main)',
        color: 'var(--color-text-primary)',
        opacity: 0,
      }}
    >
      <style>{`@keyframes fadeInDelayed { to { opacity: 1; } }`}</style>
      <div className="flex flex-col items-center gap-3">
        <Spinner
          aria-label={t('auth.restoringSession')}
          size="lg"
          style={{ color: 'var(--primary)' }}
        />
        <p className="text-sm" style={{ color: 'var(--color-text-muted)' }}>{t('auth.restoringSession')}</p>
      </div>
    </div>
  )
}
