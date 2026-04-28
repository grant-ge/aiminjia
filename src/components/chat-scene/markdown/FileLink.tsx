import { type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useNotificationStore } from '@/stores/notificationStore'
import { openFileByName } from '@/lib/tauri'

export function FileLink({ href, children }: { href?: string; children?: ReactNode }) {
  const { t } = useTranslation()
  const isFileUrl = href?.startsWith('file:///')
  const isHttp = href?.startsWith('http://') || href?.startsWith('https://')

  if (isFileUrl) {
    const fileName = (() => {
      try {
        return decodeURIComponent(href!.slice(7)).split('/').pop() ?? ''
      } catch {
        return ''
      }
    })()
    return (
      <span
        role="link"
        tabIndex={0}
        title={t('common.openFile', 'Open file')}
        style={{
          cursor: 'pointer',
          textDecoration: 'underline',
          textDecorationStyle: 'dashed',
          textUnderlineOffset: 3,
          color: 'var(--color-primary)',
        }}
        onClick={() => {
          if (!fileName) return
          openFileByName(fileName).catch(() => {
            useNotificationStore.getState().push({
              level: 'error',
              title: t('chatArea.fileNotFound', 'File not found'),
              message: t('chatArea.cannotOpenFile', { fileName, defaultValue: `Cannot open ${fileName}` }),
              actions: [],
              dismissible: true,
              autoHide: 5,
              context: 'toast',
            })
          })
        }}
      >
        {children}
      </span>
    )
  }

  if (isHttp) {
    return (
      <a
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        style={{ color: 'var(--color-primary)', textDecoration: 'underline' }}
      >
        {children}
      </a>
    )
  }

  return <>{children}</>
}
