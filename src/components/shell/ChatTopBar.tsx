/**
 * @designSource design.pen#qLmzZ
 * @sizing height 56, padding [0,24], bottom border 1, left gap 12, right gap 14
 */
import { Ellipsis, PanelLeft, Share2 } from 'lucide-react'
import type { ReactNode } from 'react'

export interface ChatTopBarEmployee {
  avatar: string
  name: string
  role: string
  onClick?: () => void
}

interface ChatTopBarProps {
  title: string
  workspace?: string
  /**
   * When set, replaces the plain title with an employee identity card
   * (avatar · name · role). Used for conversations created by employee
   * dispatch. Click handler typically opens the employee drawer.
   */
  employee?: ChatTopBarEmployee
  onShare?: () => void
  onMore?: () => void
  onToggleSidebar?: () => void
  /** extra node rendered at the right edge */
  trailing?: ReactNode
}

export function ChatTopBar({
  title,
  workspace,
  employee,
  onShare,
  onMore,
  onToggleSidebar,
  trailing,
}: ChatTopBarProps) {
  return (
    <header data-tauri-drag-region className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-background px-6">
      <div className="flex min-w-0 items-center gap-3">
        {employee ? (
          <button
            type="button"
            data-testid="chat-topbar-employee"
            onClick={employee.onClick}
            disabled={!employee.onClick}
            className="flex min-w-0 items-center gap-1.5 rounded-md px-1.5 py-0.5 text-md font-semibold text-foreground transition-colors hover:bg-accent/40 disabled:cursor-default disabled:hover:bg-transparent"
          >
            <span aria-hidden className="text-base leading-none">
              {employee.avatar}
            </span>
            <span className="truncate">{employee.name}</span>
            {employee.role ? (
              <>
                <span aria-hidden className="text-sm text-muted-foreground">·</span>
                <span className="truncate text-sm font-normal text-muted-foreground">
                  {employee.role}
                </span>
              </>
            ) : null}
          </button>
        ) : (
          <div className="truncate text-md font-semibold text-foreground">
            {title}
          </div>
        )}
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
