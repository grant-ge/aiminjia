import { useTranslation } from 'react-i18next'

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
        <div
          aria-label={t('auth.restoringSession')}
          className="h-8 w-8 animate-spin rounded-full"
          style={{
            borderStyle: 'solid',
            borderWidth: 2,
            borderRightColor: 'var(--color-border)',
            borderBottomColor: 'var(--color-border)',
            borderLeftColor: 'var(--color-border)',
            borderTopColor: 'var(--primary)',
          }}
        />
        <p className="text-sm" style={{ color: 'var(--color-text-muted)' }}>{t('auth.restoringSession')}</p>
      </div>
    </div>
  )
}
