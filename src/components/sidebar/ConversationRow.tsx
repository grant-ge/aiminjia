/**
 * @designSource design.pen#0EZDr / HsGnf / GknhC
 * @sizing padding [6,8,6,30] (indent 30 under ProjectAccordion), fontSize 13
 */
import { Archive, Copy, Ellipsis, Loader2, Pencil, Pin } from 'lucide-react'
import { useState } from 'react'

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

interface ConversationRowProps {
  id: string
  title: string
  active?: boolean
  loading?: boolean
  onClick: () => void
  onArchive?: () => void
  onRename?: () => void
}

export function ConversationRow({
  id,
  title,
  active = false,
  loading = false,
  onClick,
  onArchive,
  onRename,
}: ConversationRowProps) {
  const [hovered, setHovered] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)

  const base =
    'group relative flex w-full items-center rounded-md py-1.5 pl-[30px] pr-2 text-left text-[13px]'
  const cls = active
    ? `${base} bg-sidebar-accent text-sidebar-foreground`
    : `${base} text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40`

  const showMore = hovered || menuOpen || active

  return (
    <div
      className="relative pl-2 px-4"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <button type="button" onClick={onClick} className={cls}>
        {loading ? (
          <Loader2
            data-icon="loader"
            className="h-3.5 w-3.5 shrink-0 animate-spin text-sidebar-foreground"
          />
        ) : null}
        <span className="truncate pr-5">{title}</span>
      </button>

      {showMore && (
        <div className="absolute right-6 top-1/2 -translate-y-1/2">
          <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                onClick={(e) => e.stopPropagation()}
                className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-foreground"
              >
                <Ellipsis className="h-3.5 w-3.5" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-40 border-sidebar-border bg-sidebar p-1 text-sidebar-foreground [&_[data-highlighted]]:bg-sidebar-accent [&_[data-highlighted]]:text-sidebar-foreground">
              <DropdownMenuItem
                className="gap-2 px-3 py-1.5 text-sm opacity-40 cursor-not-allowed"
                disabled
              >
                <Pin className="h-3.5 w-3.5 shrink-0" />
                置顶聊天
              </DropdownMenuItem>
              <DropdownMenuItem
                className="gap-2 px-3 py-1.5 text-sm"
                onClick={() => onArchive?.()}
              >
                <Archive className="h-3.5 w-3.5 shrink-0" />
                归档聊天
              </DropdownMenuItem>
              <DropdownMenuItem
                className="gap-2 px-3 py-1.5 text-sm"
                onClick={() => onRename?.()}
              >
                <Pencil className="h-3.5 w-3.5 shrink-0" />
                重命名聊天
              </DropdownMenuItem>
              <DropdownMenuItem
                className="gap-2 px-3 py-1.5 text-sm"
                onClick={() => void navigator.clipboard.writeText(id)}
              >
                <Copy className="h-3.5 w-3.5 shrink-0" />
                复制会话 ID
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      )}
    </div>
  )
}
