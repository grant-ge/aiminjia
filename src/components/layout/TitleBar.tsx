import React, { useEffect, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ArrowLeft, ArrowRight, PanelLeft, PanelRight } from 'lucide-react'
import { useUiStore } from '@/stores/uiStore'
import { Button } from '@/components/ui/button'
import { syncMacTrafficLightInset } from '@/lib/tauri'

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

function useSyncMacTrafficLightInset(enabled: boolean) {
  const titleBarRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!enabled) return

    const element = titleBarRef.current
    if (!element) return

    let lastHeight = 0
    let frame = 0
    const sync = () => {
      if (frame) cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        frame = 0
        const height = element.getBoundingClientRect().height
        if (height <= 0 || Math.abs(height - lastHeight) < 0.25) return
        lastHeight = height
        void syncMacTrafficLightInset(height).catch(() => undefined)
      })
    }

    sync()

    let observer: ResizeObserver | null = null
    if (typeof ResizeObserver !== 'undefined') {
      observer = new ResizeObserver(sync)
      observer.observe(element)
    }
    window.addEventListener('resize', sync)

    return () => {
      if (frame) cancelAnimationFrame(frame)
      observer?.disconnect()
      window.removeEventListener('resize', sync)
    }
  }, [enabled])

  return titleBarRef
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

/**
 * macOS overlays native traffic lights over the same 48px header band used by
 * the page top bars. Windows still renders a compact custom title bar.
 */
interface TitleBarProps {
  appControls?: boolean
}

export function TitleBar({ appControls = true }: TitleBarProps) {
  const isMacOS = navigator.userAgent.includes('Macintosh')
  const isWindows = navigator.userAgent.includes('Windows')
  const sidebarHidden = useUiStore((s) => s.sidebarHidden)
  const reserveMacTrafficLightInset = useReserveMacTrafficLightInset(isMacOS)
  const macTitleBarRef = useSyncMacTrafficLightInset(isMacOS)

  const barClass = 'relative flex h-8 w-full shrink-0 items-center text-sidebar-foreground'
  const macDragClass = 'absolute inset-x-0 top-0 z-10 flex h-12 w-full items-center text-sidebar-foreground'
  const macControlsClass = 'pointer-events-none absolute inset-x-0 top-0 z-30 flex h-12 w-full items-center justify-between text-sidebar-foreground'
  const barStyle = isMacOS ? OVERLAY_TITLE_BAR_STYLE : SIDEBAR_TITLE_BAR_STYLE
  const leftGroupClass = sidebarHidden
    ? 'flex items-center pl-2'
    : 'flex w-64 items-center justify-end pr-2'
  const windowsLeftGroupClass = 'flex items-center pl-2'
  const macLeftGroupClass = sidebarHidden
    ? reserveMacTrafficLightInset
      ? 'flex items-center pl-20'
      : leftGroupClass
    : leftGroupClass
  const macButtonGroupClass = reserveMacTrafficLightInset && sidebarHidden
    ? 'pointer-events-auto ml-2 flex items-center'
    : 'pointer-events-auto flex items-center'

  if (isMacOS) {
    if (!appControls) {
      return (
        <div
          ref={macTitleBarRef}
          data-tauri-drag-region
          className={macDragClass}
          style={barStyle}
        />
      )
    }

    return (
      <>
        <div
          ref={macTitleBarRef}
          data-tauri-drag-region
          className={macDragClass}
          style={barStyle}
        />
        <div className={macControlsClass}>
          <div className={macLeftGroupClass}>
            <div className={macButtonGroupClass}>
              <SidebarToggleButton />
              <TitleBarNavigationButtons />
            </div>
          </div>
        </div>
      </>
    )
  }

  if (!isWindows) {
    if (!appControls) {
      return (
        <div
          data-tauri-drag-region
          className={`${barClass} justify-end`}
          style={barStyle}
        />
      )
    }

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
      {appControls ? (
        <div className={windowsLeftGroupClass}>
          <SidebarToggleButton />
          <TitleBarNavigationButtons />
        </div>
      ) : null}
      <div className="flex-1" data-tauri-drag-region />
      <WindowControls />
    </div>
  )
}
