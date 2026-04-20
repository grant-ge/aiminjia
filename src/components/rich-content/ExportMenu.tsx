/**
 * ExportMenu — dropdown button for multi-format conversation export.
 * Supports HTML, PDF (via Tauri IPC), PPT (pptxgenjs), and Excel (xlsx).
 */
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { exportConversation, openGeneratedFile } from '@/lib/tauri'
import { exportAsPptx } from '@/lib/pptxExport'
import { exportAsExcel } from '@/lib/excelExport'
import type { MessageContent } from '@/types/message'

interface ExportMenuProps {
  conversationId: string
  disabled?: boolean
}

type ExportFormat = 'pdf' | 'html' | 'pptx' | 'xlsx'

interface FormatOption {
  format: ExportFormat
  labelKey: string
  icon: string
}

const FORMAT_OPTIONS: FormatOption[] = [
  { format: 'pdf', labelKey: 'topBar.exportAsPdf', icon: 'PDF' },
  { format: 'html', labelKey: 'topBar.exportAsHtml', icon: 'HTML' },
  { format: 'pptx', labelKey: 'topBar.exportAsPptx', icon: 'PPT' },
  { format: 'xlsx', labelKey: 'topBar.exportAsXlsx', icon: 'XLS' },
]

const ICON_COLORS: Record<string, { bg: string; fg: string }> = {
  PDF: { bg: 'var(--color-filetype-red-bg, #FEE2E2)', fg: 'var(--color-semantic-red, #EF4444)' },
  HTML: { bg: 'var(--color-filetype-blue-bg, #DBEAFE)', fg: 'var(--color-semantic-blue, #3B82F6)' },
  PPT: { bg: 'var(--color-filetype-orange-bg, #FED7AA)', fg: 'var(--color-semantic-orange, #F97316)' },
  XLS: { bg: 'var(--color-filetype-green-bg, #D1FAE5)', fg: 'var(--color-semantic-green, #10B981)' },
}

export function ExportMenu({ conversationId, disabled }: ExportMenuProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [exporting, setExporting] = useState(false)
  const dropdownRef = useRef<HTMLDivElement>(null)
  const messages = useChatStore((s) => s.messages)
  const conversations = useChatStore((s) => s.conversations)

  // Close on outside click
  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  const conversation = conversations.find((c) => c.id === conversationId)
  const title = conversation?.title ?? 'Export'

  const handleExport = async (format: ExportFormat) => {
    setOpen(false)
    setExporting(true)

    try {
      if (format === 'pdf' || format === 'html') {
        // Existing Tauri IPC export
        const result = await exportConversation(conversationId, format)
        if (result.wasFallback) {
          // PDF was requested but the bundled runtime cannot produce it —
          // surface this explicitly instead of silently delivering HTML.
          useNotificationStore.getState().push({
            level: 'warning',
            title: t('topBar.exportPdfFallback'),
            message: t('topBar.exportPdfFallbackHint', { fileName: result.fileName }),
            actions: [],
            dismissible: true,
            autoHide: 10,
            context: 'toast',
          })
        } else {
          useNotificationStore.getState().push({
            level: 'success',
            title: t('topBar.exportSuccess'),
            message: `${result.fileName} ${t('topBar.saved')}`,
            actions: [],
            dismissible: true,
            autoHide: 5,
            context: 'toast',
          })
        }
        await openGeneratedFile(result.fileId, conversationId)
      } else if (format === 'pptx') {
        // Client-side PPT generation — save via Tauri fs plugin
        const { save } = await import('@tauri-apps/plugin-dialog')
        const { writeFile } = await import('@tauri-apps/plugin-fs')

        const contents: MessageContent[] = messages
          .filter((m) => m.role === 'assistant')
          .map((m) => m.content)

        const buffer = await exportAsPptx(title, contents)
        const filename = title.replace(/[<>:"/\\|?*]/g, '_') || 'export'
        const path = await save({
          defaultPath: `${filename}.pptx`,
          filters: [{ name: 'PowerPoint', extensions: ['pptx'] }],
        })
        if (!path) return

        await writeFile(path, new Uint8Array(buffer))

        useNotificationStore.getState().push({
          level: 'success',
          title: t('topBar.exportSuccess'),
          message: `${filename}.pptx ${t('topBar.saved')}`,
          actions: [],
          dismissible: true,
          autoHide: 5,
          context: 'toast',
        })
      } else if (format === 'xlsx') {
        // Client-side Excel generation — save via Tauri fs plugin
        const { save } = await import('@tauri-apps/plugin-dialog')
        const { writeFile } = await import('@tauri-apps/plugin-fs')

        const allTables = messages
          .filter((m) => m.role === 'assistant')
          .flatMap((m) => m.content.tables ?? [])

        const buffer = await exportAsExcel(title, allTables)
        const filename = title.replace(/[<>:"/\\|?*]/g, '_') || 'export'
        const path = await save({
          defaultPath: `${filename}.xlsx`,
          filters: [{ name: 'Excel', extensions: ['xlsx'] }],
        })
        if (!path) return

        await writeFile(path, buffer)

        useNotificationStore.getState().push({
          level: 'success',
          title: t('topBar.exportSuccess'),
          message: `${filename}.xlsx ${t('topBar.saved')}`,
          actions: [],
          dismissible: true,
          autoHide: 5,
          context: 'toast',
        })
      }
    } catch (err) {
      console.error('Export failed:', err)
      useNotificationStore.getState().push({
        level: 'error',
        title: t('topBar.exportFailed'),
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    } finally {
      setExporting(false)
    }
  }

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        className="flex cursor-pointer items-center gap-1 rounded-md border py-1 px-2 transition-all duration-150"
        style={{
          fontSize: 'var(--text-xs)',
          background: 'transparent',
          borderColor: 'var(--color-border)',
          color: 'var(--color-text-muted)',
        }}
        title={t('topBar.exportConversation')}
        disabled={disabled ?? exporting}
        onClick={() => setOpen((prev) => !prev)}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = 'var(--color-bg-card-hover)'
          e.currentTarget.style.color = 'var(--color-text-secondary)'
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = 'transparent'
          e.currentTarget.style.color = 'var(--color-text-muted)'
        }}
      >
        {exporting ? (
          <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" />
        ) : (
          <svg
            className="h-3.5 w-3.5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4" />
            <polyline points="7 10 12 15 17 10" />
            <line x1="12" y1="15" x2="12" y2="3" />
          </svg>
        )}
        <span>{t('topBar.export')}</span>
      </button>

      {open && (
        <div
          className="absolute right-0 top-full z-50 mt-1 min-w-[180px] overflow-hidden rounded-lg border"
          style={{
            background: 'var(--color-bg-card)',
            borderColor: 'var(--color-border)',
            boxShadow: 'var(--shadow-modal)',
          }}
        >
          <div className="py-1">
            {FORMAT_OPTIONS.map((opt) => {
              const iconStyle = ICON_COLORS[opt.icon]
              return (
                <button
                  key={opt.format}
                  className="flex w-full cursor-pointer items-center gap-2.5 border-none px-3 py-2 text-sm transition-colors duration-100"
                  style={{ background: 'transparent', color: 'var(--color-text-secondary)' }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = 'var(--color-bg-card-hover)'
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = 'transparent'
                  }}
                  onClick={() => handleExport(opt.format)}
                >
                  <span
                    className="inline-flex h-6 w-8 items-center justify-center rounded text-xs font-bold"
                    style={{ background: iconStyle?.bg, color: iconStyle?.fg }}
                  >
                    {opt.icon}
                  </span>
                  <span>{t(opt.labelKey)}</span>
                </button>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
