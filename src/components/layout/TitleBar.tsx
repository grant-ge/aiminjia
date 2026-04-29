import React from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

function handleDragStart(e: React.MouseEvent) {
  if (e.buttons === 1 && e.detail === 2) {
    void getCurrentWindow().toggleMaximize()
    return
  }
  if (e.buttons === 1 && e.detail === 1) {
    getCurrentWindow().startDragging()
  }
}

function WindowControls() {
  const win = getCurrentWindow()
  const btnClass =
    'flex h-7 w-11 items-center justify-center text-sidebar-foreground/40 transition-colors hover:bg-sidebar-accent hover:text-sidebar-foreground/80'
  return (
    <div className="flex shrink-0">
      <button className={btnClass} onClick={() => win.minimize()} aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>
      </button>
      <button className={btnClass} onClick={() => win.toggleMaximize()} aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>
      </button>
      <button
        className={`${btnClass} hover:!bg-red-600 hover:!text-white`}
        onClick={() => win.close()}
        aria-label="Close"
      >
        <svg width="10" height="10" viewBox="0 0 10 10"><path fill="currentColor" d="M1.7.3.3 1.7 3.6 5 .3 8.3l1.4 1.4L5 6.4l3.3 3.3 1.4-1.4L6.4 5l3.3-3.3L8.3.3 5 3.6 1.7.3z"/></svg>
      </button>
    </div>
  )
}

/** Renders only on Windows. macOS keeps the sidebar-top drag spacer in AppSidebar. */
export function TitleBar() {
  if (!navigator.userAgent.includes('Windows')) return null
  return (
    <div
      data-tauri-drag-region
      className="flex h-7 w-full shrink-0 items-center bg-sidebar"
      onMouseDown={handleDragStart}
    >
      <div className="flex-1" data-tauri-drag-region />
      <WindowControls />
    </div>
  )
}
