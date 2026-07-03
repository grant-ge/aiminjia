import React, { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ArrowLeft, ArrowRight, PanelLeft, PanelRight } from 'lucide-react'
import { UpdateAvailableLink } from './UpdateAvailableLink'
import { TitleBarEnvSwitcher } from './TitleBarEnvSwitcher'
import { useUpdaterStore } from '@/lib/updaterStore'
import { useUiStore } from '@/stores/uiStore'
import { Tag } from '@/components/common/Tag'
import { Button } from '@/components/ui/button'

function handleDragStart(e: React.MouseEvent) {
  if (e.buttons === 1 && e.detail === 2) {
    void getCurrentWindow().toggleMaximize()
    return
  }
  if (e.buttons === 1 && e.detail === 1) {
    getCurrentWindow().startDragging()
  }
}

function useReserveMacTrafficLightInset(enabled: boolean) {
  const [isFullscreen, setIsFullscreen] = useState(false)

  useEffect(() => {
    if (!enabled) {
      setIsFullscreen(false)
      return
    }

    const win = getCurrentWindow()
    let cancelled = false
    let unlisten: (() => void) | null = null

    const syncFullscreen = () => {
      void win.isFullscreen()
        .then((next) => {
          if (!cancelled) setIsFullscreen(next)
        })
        .catch(() => {
          if (!cancelled) setIsFullscreen(false)
        })
    }

    syncFullscreen()
    void win.onResized(() => {
      syncFullscreen()
    }).then((nextUnlisten) => {
      if (cancelled) {
        nextUnlisten()
        return
      }
      unlisten = nextUnlisten
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [enabled])

  return enabled && !isFullscreen
}

function WindowControls() {
  // Keep window controls readable on the sidebar-colored title bar; close
  // button hover routes to --destructive instead of hardcoded red.
  return (
    <div className="flex shrink-0 items-center" onMouseDown={(e) => e.stopPropagation()}>
      <Button
        link
        type="button"
        className="titlebar-window-button"
        icon={<svg className="h-2.5 w-2.5" width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>}
        onClick={() => getCurrentWindow().minimize()}
        aria-label="Minimize"
      />
      <Button
        link
        type="button"
        className="titlebar-window-button"
        icon={<svg className="h-2.5 w-2.5" width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>}
        onClick={() => getCurrentWindow().toggleMaximize()}
        aria-label="Maximize"
      />
      <Button
        link
        type="button"
        className="titlebar-window-button titlebar-window-button-close"
        icon={<svg className="h-2.5 w-2.5" width="10" height="10" viewBox="0 0 10 10"><path fill="currentColor" d="M1.7.3.3 1.7 3.6 5 .3 8.3l1.4 1.4L5 6.4l3.3 3.3 1.4-1.4L6.4 5l3.3-3.3L8.3.3 5 3.6 1.7.3z"/></svg>}
        onClick={() => getCurrentWindow().close()}
        aria-label="Close"
      />
    </div>
  )
}

function SidebarToggleButton({ className = '' }: { className?: string }) {
  const sidebarHidden = useUiStore((s) => s.sidebarHidden)
  const toggleSidebarHidden = useUiStore((s) => s.toggleSidebarHidden)
  const Icon = sidebarHidden ? PanelRight : PanelLeft
  const label = sidebarHidden ? '显示侧栏' : '隐藏侧栏'

  return (
    <Button
      link
      type="button"
      data-aijia-sidebar-toggle="true"
      aria-label={label}
      title={label}
      className={`titlebar-sidebar-toggle ${className}`}
      icon={<Icon className="h-4 w-4" aria-hidden="true" />}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation()
        toggleSidebarHidden()
      }}
    />
  )
}

function TitleBarNavigationButtons() {
  const canGoBack = useUiStore((s) => s.canGoBack())
  const canGoForward = useUiStore((s) => s.canGoForward())
  const goBack = useUiStore((s) => s.goBack)
  const goForward = useUiStore((s) => s.goForward)

  return (
    <div className="ml-2 flex items-center" onMouseDown={(e) => e.stopPropagation()}>
      <Button
        link
        type="button"
        aria-label="后退"
        title="后退"
        className="titlebar-navigation-button"
        disabled={!canGoBack}
        icon={<ArrowLeft className="h-4 w-4" aria-hidden="true" />}
        onClick={(e) => {
          e.stopPropagation()
          goBack()
        }}
      />
      <Button
        link
        type="button"
        aria-label="前进"
        title="前进"
        className="titlebar-navigation-button"
        disabled={!canGoForward}
        icon={<ArrowRight className="h-4 w-4" aria-hidden="true" />}
        onClick={(e) => {
          e.stopPropagation()
          goForward()
        }}
      />
    </div>
  )
}

const SIDEBAR_TITLE_BAR_STYLE: React.CSSProperties = {
  backgroundColor: 'var(--sidebar)',
}

const OVERLAY_TITLE_BAR_STYLE: React.CSSProperties = {
  backgroundColor: 'transparent',
}

// "DEV" or "DEV 5174" when a vite dev port is detectable.  Including the port
// makes multi-instance dev (two vite servers side by side) visually
// distinguishable.  Pass an explicit `port` for tests; otherwise reads
// `window.location.port` at call time.
export function getDevBadgeLabel(port?: string): string {
  const detected = port ?? (typeof window !== 'undefined' ? window.location.port : '')
  return detected ? `DEV ${detected}` : 'DEV'
}

// DEV badge: not tenant-themed by design (it's a build-mode marker, not UI).
// Color picked from semantic blue so it stays distinct from any tenant accent.
function DevBadge() {
  return (
    <Tag
      size="xs"
      variant="solid"
      color="primary"
      className="pointer-events-none mr-2 border-transparent bg-[var(--color-semantic-purple)] tracking-widest shadow-[var(--shadow-sm)]"
    >
      {getDevBadgeLabel()}
    </Tag>
  )
}

/**
 * macOS overlays native traffic lights over the same 48px header band used by
 * the page top bars. Windows still renders a compact custom title bar.
 */
export function TitleBar() {
  const showUpdateLink = useUpdaterStore((s) =>
    s.phase === 'available' || s.phase === 'downloading' || s.phase === 'ready' || s.phase === 'failed'
  )
  const isMacOS = navigator.userAgent.includes('Macintosh')
  const isWindows = navigator.userAgent.includes('Windows')
  const isDev = import.meta.env.DEV
  const sidebarHidden = useUiStore((s) => s.sidebarHidden)
  const reserveMacTrafficLightInset = useReserveMacTrafficLightInset(isMacOS)

  const barClass = 'flex h-8 w-full shrink-0 items-center text-sidebar-foreground'
  const macDragClass = 'absolute inset-x-0 top-0 z-10 flex h-12 w-full items-center text-sidebar-foreground'
  const macControlsClass = 'pointer-events-none absolute inset-x-0 top-0 z-30 flex h-12 w-full items-center justify-between text-sidebar-foreground'
  const barStyle = isMacOS ? OVERLAY_TITLE_BAR_STYLE : SIDEBAR_TITLE_BAR_STYLE
  const leftGroupClass = sidebarHidden
    ? 'flex items-center pl-2'
    : 'flex w-64 items-center justify-end pr-2'
  const macLeftGroupClass = sidebarHidden
    ? reserveMacTrafficLightInset
      ? 'flex items-center pl-20'
      : leftGroupClass
    : leftGroupClass

  if (isMacOS) {
    return (
      <>
        <div
          data-tauri-drag-region
          className={macDragClass}
          style={barStyle}
        />
        <div className={macControlsClass}>
          <div className={macLeftGroupClass}>
            <div className="pointer-events-auto flex items-center">
              <SidebarToggleButton />
              <TitleBarNavigationButtons />
            </div>
          </div>
          <div className="pointer-events-auto flex items-center">
            {showUpdateLink ? (
              <div className="pr-3" onMouseDown={(e) => e.stopPropagation()}>
                <UpdateAvailableLink />
              </div>
            ) : null}
            {isDev ? <TitleBarEnvSwitcher /> : null}
            {isDev ? <DevBadge /> : null}
          </div>
        </div>
      </>
    )
  }

  if (!isWindows) {
    return (
      <div
        data-tauri-drag-region
        className={`${barClass} justify-between`}
        style={barStyle}
      >
        <div className={leftGroupClass}>
          <SidebarToggleButton />
          <TitleBarNavigationButtons />
        </div>
        <div className="flex items-center">
          {showUpdateLink ? (
            <div className="pr-3" onMouseDown={(e) => e.stopPropagation()}>
              <UpdateAvailableLink />
            </div>
          ) : null}
          {isDev ? <TitleBarEnvSwitcher /> : null}
          {isDev ? <DevBadge /> : null}
        </div>
      </div>
    )
  }

  return (
    <div
      data-tauri-drag-region
      className={barClass}
      style={barStyle}
      onMouseDown={handleDragStart}
    >
      <div className={leftGroupClass}>
        <SidebarToggleButton />
        <TitleBarNavigationButtons />
      </div>
      <div className="flex-1" data-tauri-drag-region />
      <div onMouseDown={(e) => e.stopPropagation()}>
        <UpdateAvailableLink />
      </div>
      {isDev ? <TitleBarEnvSwitcher /> : null}
      {isDev ? <DevBadge /> : null}
      <WindowControls />
    </div>
  )
}
