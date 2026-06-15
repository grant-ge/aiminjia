import React from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ArrowLeft, ArrowRight, PanelLeft, PanelRight } from 'lucide-react'
import { UpdateAvailableLink } from './UpdateAvailableLink'
import { TitleBarEnvSwitcher } from './TitleBarEnvSwitcher'
import { useUpdaterStore } from '@/lib/updaterStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore } from '@/stores/uiStore'
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

function WindowControls() {
  // Keep window controls readable on the sidebar-colored title bar; close
  // button hover routes to --destructive instead of hardcoded red.
  return (
    <div className="flex shrink-0 items-center" onMouseDown={(e) => e.stopPropagation()}>
      <Button link type="button" className="titlebar-window-button" onClick={() => getCurrentWindow().minimize()} aria-label="Minimize">
        <svg width="10" height="1" viewBox="0 0 10 1"><rect fill="currentColor" width="10" height="1"/></svg>
      </Button>
      <Button link type="button" className="titlebar-window-button" onClick={() => getCurrentWindow().toggleMaximize()} aria-label="Maximize">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect fill="none" stroke="currentColor" strokeWidth="1" x="0.5" y="0.5" width="9" height="9"/></svg>
      </Button>
      <Button
        link
        type="button"
        className="titlebar-window-button titlebar-window-button-close"
        onClick={() => getCurrentWindow().close()}
        aria-label="Close"
      >
        <svg width="10" height="10" viewBox="0 0 10 10"><path fill="currentColor" d="M1.7.3.3 1.7 3.6 5 .3 8.3l1.4 1.4L5 6.4l3.3 3.3 1.4-1.4L6.4 5l3.3-3.3L8.3.3 5 3.6 1.7.3z"/></svg>
      </Button>
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
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation()
        toggleSidebarHidden()
      }}
    >
      <Icon className="h-4 w-4" aria-hidden="true" />
    </Button>
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
        onClick={(e) => {
          e.stopPropagation()
          goBack()
        }}
      >
        <ArrowLeft className="h-4 w-4" aria-hidden="true" />
      </Button>
      <Button
        link
        type="button"
        aria-label="前进"
        title="前进"
        className="titlebar-navigation-button"
        disabled={!canGoForward}
        onClick={(e) => {
          e.stopPropagation()
          goForward()
        }}
      >
        <ArrowRight className="h-4 w-4" aria-hidden="true" />
      </Button>
    </div>
  )
}

function CompactTenantBrand() {
  const productName = useBrandingStore((s) => s.productName)
  const logoUrl = useBrandingStore((s) => s.logoUrl)

  return (
    <div
      data-testid="titlebar-tenant-brand"
      data-tauri-drag-region
      className="ml-2 flex h-7 max-w-[140px] shrink-0 select-none items-center gap-1.5 overflow-hidden rounded-md px-1.5 text-sidebar-foreground"
      title={productName}
    >
      <span className="h-5 w-5 shrink-0 overflow-hidden rounded border border-sidebar-border bg-card">
        <img src={logoUrl} alt="Brand logo" className="h-full w-full object-cover" />
      </span>
      <span className="truncate text-xs font-semibold">{productName}</span>
    </div>
  )
}

/**
 * The native drag strip uses the same surface color as the left sidebar so the
 * window chrome and app navigation read as one continuous shell.
 */
const TITLE_BAR_STYLE: React.CSSProperties = {
  backgroundColor: 'var(--sidebar)',
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
    <span
      className="pointer-events-none mr-2 rounded-md bg-[var(--color-semantic-purple)] px-1.5 py-0.5 text-[11px] font-semibold tracking-widest text-primary-foreground shadow-[var(--shadow-sm)]"
    >
      {getDevBadgeLabel()}
    </span>
  )
}

/**
 * Both macOS (Overlay titleBarStyle) and Windows render a 28px shell strip at
 * the top. macOS draws native traffic lights over this strip.
 */
export function TitleBar() {
  const showUpdateLink = useUpdaterStore((s) =>
    s.phase === 'available' || s.phase === 'downloading' || s.phase === 'ready' || s.phase === 'failed'
  )
  const isWindows = navigator.userAgent.includes('Windows')
  const isDev = import.meta.env.DEV

  const barClass = 'flex h-8 w-full shrink-0 items-center text-sidebar-foreground'
  const barStyle = TITLE_BAR_STYLE

  if (!isWindows) {
    return (
      <div
        data-tauri-drag-region
        className={`${barClass} justify-between`}
        style={barStyle}
      >
        <div className="flex items-center pl-20">
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
      <CompactTenantBrand />
      <SidebarToggleButton className="ml-2" />
      <TitleBarNavigationButtons />
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
