/**
 * TopBar — title + export button. Hidden when no active conversation.
 * Based on visual-prototype-zh.html top-bar section.
 */
import { useEffect, useRef, useState } from 'react'
import { useChatStore } from '@/stores/chatStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { exportConversation, openGeneratedFile } from '@/lib/tauri'

export function TopBar() {
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const conversations = useChatStore((s) => s.conversations)
  const streamStates = useChatStore((s) => s.streamStates)
  const isStreaming = activeConversationId ? (streamStates[activeConversationId]?.isStreaming ?? false) : false
  const [exportDropdownOpen, setExportDropdownOpen] = useState(false)
  const [exporting, setExporting] = useState(false)
  const exportDropdownRef = useRef<HTMLDivElement>(null)

  const activeConversation = conversations.find(
    (c) => c.id === activeConversationId,
  )

  // Click-outside to close export dropdown
  useEffect(() => {
    if (!exportDropdownOpen) return
    const handleMouseDown = (e: MouseEvent) => {
      if (exportDropdownRef.current && !exportDropdownRef.current.contains(e.target as Node)) {
        setExportDropdownOpen(false)
      }
    }
    document.addEventListener('mousedown', handleMouseDown)
    return () => document.removeEventListener('mousedown', handleMouseDown)
  }, [exportDropdownOpen])

  // Hide TopBar when no active conversation (welcome screen)
  if (!activeConversation) return null

  const title = activeConversation.title

  const handleExport = async (format: 'pdf' | 'html') => {
    setExportDropdownOpen(false)
    if (!activeConversationId) return

    setExporting(true)
    try {
      const result = await exportConversation(activeConversationId, format)
      useNotificationStore.getState().push({
        level: 'success',
        title: '导出成功',
        message: `${result.fileName} 已保存`,
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
      await openGeneratedFile(result.fileId, activeConversationId)
    } catch (err) {
      console.error('Export failed:', err)
      useNotificationStore.getState().push({
        level: 'error',
        title: '导出失败',
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

  const showExportButton = !isStreaming

  return (
    <header
      className="flex h-11 shrink-0 items-center border-b px-6"
      style={{ borderColor: 'var(--color-border)' }}
    >
      <h2
        className="text-base font-semibold truncate"
        style={{ color: 'var(--color-text-primary)' }}
      >
        {title}
      </h2>

      <div className="ml-auto flex items-center gap-2">
        {showExportButton && (
          <div className="relative" ref={exportDropdownRef}>
            <button
              className="flex cursor-pointer items-center gap-1 rounded-md border py-1 px-2 transition-all duration-150"
              style={{
                fontSize: 'var(--text-xs)',
                background: 'transparent',
                borderColor: 'var(--color-border)',
                color: 'var(--color-text-muted)',
              }}
              title="导出对话"
              disabled={exporting}
              onClick={() => setExportDropdownOpen((prev) => !prev)}
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
                <span
                  className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent"
                />
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
              <span>导出</span>
            </button>

            {exportDropdownOpen && (
              <div
                className="absolute right-0 top-full z-50 mt-1 min-w-[160px] overflow-hidden rounded-lg border"
                style={{
                  background: 'var(--color-bg-card)',
                  borderColor: 'var(--color-border)',
                  boxShadow: 'var(--shadow-modal)',
                }}
              >
                <div className="py-1">
                  <button
                    className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors duration-100"
                    style={{ background: 'transparent', color: 'var(--color-text-secondary)' }}
                    onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-bg-card-hover)' }}
                    onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent' }}
                    onClick={() => handleExport('pdf')}
                  >
                    <svg className="h-4 w-4 opacity-60" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zM6 20V4h7v5h5v11H6z" />
                    </svg>
                    <span>导出为 PDF</span>
                  </button>
                  <button
                    className="flex w-full cursor-pointer items-center gap-2 border-none px-3 py-2 text-sm transition-colors duration-100"
                    style={{ background: 'transparent', color: 'var(--color-text-secondary)' }}
                    onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--color-bg-card-hover)' }}
                    onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent' }}
                    onClick={() => handleExport('html')}
                  >
                    <svg className="h-4 w-4 opacity-60" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zM6 20V4h7v5h5v11H6z" />
                    </svg>
                    <span>导出为 HTML</span>
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </header>
  )
}
