/**
 * ToastContainer — renders notification toasts from the notification store.
 * Positioned fixed at the bottom-right of the viewport.
 * White card bg + left semantic-color border for clean, minimal appearance.
 */
import { useNotificationStore } from '@/stores/notificationStore'
import type { Notification, NotificationLevel } from '@/stores/notificationStore'
import { Button } from '@/components/ui/button'

const LEVEL_STYLES: Record<NotificationLevel, { accent: string; icon: string }> = {
  error: {
    accent: 'var(--color-semantic-red)',
    icon: '!',
  },
  warning: {
    accent: 'var(--color-semantic-orange)',
    icon: '!',
  },
  success: {
    accent: 'var(--color-semantic-green)',
    icon: '\u2713',
  },
  info: {
    accent: 'var(--color-semantic-blue)',
    icon: 'i',
  },
}

function Toast({ notification }: { notification: Notification }) {
  const dismiss = useNotificationStore((s) => s.dismiss)
  const style = LEVEL_STYLES[notification.level]

  return (
    <div
      className="animate-[fadeUp_0.25s_ease] flex w-[360px] max-w-[calc(100vw-32px)] items-start gap-2.5 rounded-md border border-l-[3px] px-4 py-3"
      style={{
        background: 'var(--color-bg-card)',
        borderColor: 'var(--color-border)',
        borderLeftColor: style.accent,
        boxShadow: 'var(--shadow-popover)',
      }}
    >
      {/* Icon */}
      {/* level 图标 chip：背景是 style.accent（success/error/info 的实色），白字保持对比度 */}
      <div
        className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-xs font-bold text-white"
        style={{ background: style.accent }}
      >
        {style.icon}
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        <div
          className="text-sm font-semibold"
          style={{ color: 'var(--color-text-primary)' }}
        >
          {notification.title}
        </div>
        <div
          className="mt-0.5 text-xs leading-relaxed break-words"
          style={{ color: 'var(--color-text-secondary)' }}
        >
          {notification.message}
        </div>
      </div>

      {/* Dismiss */}
      {notification.dismissible && (
        <Button unstyled
          className="flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded border-none bg-transparent transition-colors hover:bg-muted"
          style={{ color: 'var(--color-text-muted)' }}
          onClick={() => dismiss(notification.id)}
        >
          <svg className="h-3 w-3" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
          </svg>
        </Button>
      )}
    </div>
  )
}

export function ToastContainer() {
  const notifications = useNotificationStore((s) => s.notifications)
  const toasts = notifications.filter((n) => n.context === 'toast')

  if (toasts.length === 0) return null

  return (
    <div className="fixed bottom-4 right-4 z-[9999] flex flex-col-reverse gap-2">
      {toasts.map((n) => (
        <Toast key={n.id} notification={n} />
      ))}
    </div>
  )
}
