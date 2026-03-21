/**
 * BrowserPanel — shows the state of the embedded browser WebView.
 * Displayed alongside ChatArea when a browser session is active.
 */
import { invoke } from '@tauri-apps/api/core'
import { useBrowserStore } from '@/stores/browserStore'

export function BrowserPanel() {
  const { isOpen, isLoading, url, title, appId } = useBrowserStore()

  if (!isOpen) return null

  const handleShowWindow = async () => {
    try {
      await invoke('show_browse_view')
    } catch (err) {
      console.error('Failed to show browse window:', err)
    }
  }

  const handleClose = () => {
    // Just hide the panel — the WebView stays alive for future AI browse calls.
    // The WebView itself is a separate window; it will be closed when the user disconnects the app.
    useBrowserStore.getState().reset()
  }

  return (
    <div className="flex w-[320px] shrink-0 flex-col border-l border-[var(--color-border)]">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3 py-2">
        <div className="flex-1 min-w-0">
          <div className="truncate text-xs text-[var(--color-text-muted)]">{url || 'about:blank'}</div>
        </div>
        <button
          onClick={handleShowWindow}
          className="shrink-0 rounded px-2 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-card)] transition-colors"
          title="Show browser window"
        >
          ↗
        </button>
        <button
          onClick={handleClose}
          className="shrink-0 rounded px-1.5 py-1 text-xs text-[var(--color-text-muted)] hover:bg-[var(--color-bg-card)] transition-colors"
          title="Close panel"
        >
          ✕
        </button>
      </div>

      {/* Status */}
      <div className="flex-1 flex items-center justify-center p-4">
        <div className="text-center">
          {isLoading ? (
            <>
              <div className="mb-2 text-lg animate-spin inline-block">⟳</div>
              <div className="text-sm text-[var(--color-text-muted)]">Loading...</div>
            </>
          ) : (
            <>
              <div className="mb-2 text-sm font-medium text-[var(--color-text-primary)]">
                {title || 'Browser'}
              </div>
              <div className="text-xs text-[var(--color-text-muted)]">
                Browser window is open.
              </div>
              <button
                onClick={handleShowWindow}
                className="mt-3 rounded-md bg-[var(--color-primary)] px-3 py-1.5 text-xs text-white hover:opacity-90 transition-opacity"
              >
                Show Window
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
