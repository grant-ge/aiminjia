/**
 * @designSource design.pen#qLmzZ
 * @sizing height 56, padding [0,24], bottom border 1, left gap 12, right gap 14
 */
import { Ellipsis, PanelLeft, Share2 } from 'lucide-react'
import type { ReactNode } from 'react'

interface ChatTopBarProps {
  title: string
  workspace?: string
  onShare?: () => void
  onMore?: () => void
  onToggleSidebar?: () => void
  /** extra node rendered at the right edge */
  trailing?: ReactNode
}

export function ChatTopBar({
  title,
  workspace,
  onShare,
  onMore,
  onToggleSidebar,
  trailing,
}: ChatTopBarProps) {
  return (
    <header data-tauri-drag-region className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-background px-6">
      <div className="flex min-w-0 items-center gap-3">
        <div className="truncate text-md font-semibold text-foreground">
          {title}
        </div>
        {workspace ? (
          <>
            <span className="text-sm text-muted-foreground">/</span>
            <span className="truncate text-sm text-muted-foreground">
              {workspace}
            </span>
          </>
        ) : null}
      </div>
      <div className="flex items-center gap-4">
        {trailing}
        {onShare ? (
          <button
            type="button"
            aria-label="分享"
            onClick={onShare}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <Share2 className="h-4 w-4" />
          </button>
        ) : null}
        {onMore ? (
          <button
            type="button"
            aria-label="更多"
            onClick={onMore}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <Ellipsis className="h-4 w-4" />
          </button>
        ) : null}
        {onToggleSidebar ? (
          <button
            type="button"
            aria-label="折叠侧栏"
            onClick={onToggleSidebar}
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <PanelLeft className="h-4 w-4" />
          </button>
        ) : null}
      </div>
    </header>
  )
}
