/**
 * @designSource design.pen#v46uG
 * @sizing r-14 border 1 bg card padding [14,16] gap 16; fileIcon 44×52 r-6 bg muted
 */
import type { ReactNode } from 'react'
import { ChevronDown, ExternalLink, Eye, FolderOpen } from 'lucide-react'

import type { GeneratedFilePrimaryAction } from '@/components/chat/generatedFileActions'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

interface GeneratedFileCardProps {
  title: string
  sub: string
  appName: string
  fileIcon?: ReactNode
  appIcon?: ReactNode
  primaryAction?: GeneratedFilePrimaryAction
  canPreview?: boolean
  canOpenExternal?: boolean
  canReveal?: boolean
  onOpen?: () => void
  onPreview?: () => void
  onOpenExternal?: () => void
  onReveal?: () => void
}

export function GeneratedFileCard({
  title,
  sub,
  appName,
  fileIcon,
  appIcon,
  primaryAction = 'open',
  canPreview = false,
  canOpenExternal = true,
  canReveal = true,
  onOpen,
  onPreview,
  onOpenExternal,
  onReveal,
}: GeneratedFileCardProps) {
  const openExternalAction = onOpenExternal ?? onOpen
  const previewEnabled = canPreview && Boolean(onPreview)
  const openEnabled = canOpenExternal !== false && Boolean(openExternalAction)
  const revealEnabled = canReveal !== false && Boolean(onReveal)
  const isPreviewPrimary = primaryAction === 'preview'
  const primaryLabel = isPreviewPrimary ? 'Preview' : 'Open'
  const isPrimaryDisabled = isPreviewPrimary ? !previewEnabled : !openEnabled

  const handlePrimaryAction = () => {
    if (isPreviewPrimary) {
      if (previewEnabled) onPreview()
      return
    }
    if (openEnabled) openExternalAction?.()
  }

  return (
    <div className="flex items-center justify-between gap-4 rounded-[14px] border border-border bg-card p-4">
      <div className="flex min-w-0 items-center gap-3">
        <div className="flex h-[52px] w-11 items-center justify-center rounded-md border border-border bg-muted">
          {fileIcon}
        </div>
        <div className="flex min-w-0 flex-col gap-1">
          <div className="truncate text-[15px] font-semibold text-foreground">{title}</div>
          <div className="truncate text-[13px] text-muted-foreground">{sub}</div>
        </div>
      </div>
      <div className="flex shrink-0 items-center rounded-full border border-border bg-background text-[13px] text-foreground shadow-sm">
        <button
          type="button"
          onClick={handlePrimaryAction}
          disabled={isPrimaryDisabled}
          aria-label={`${primaryLabel} ${title}`}
          className="flex items-center gap-2 rounded-l-full py-1.5 pl-3 pr-2 transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
        >
          {appIcon}
          <span>{appName}</span>
        </button>
        <span className="h-4 w-px bg-border" />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={`More actions for ${title}`}
              className="flex items-center rounded-r-full py-1.5 pl-2 pr-2 transition-colors hover:bg-muted"
            >
              <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="min-w-52">
            <DropdownMenuItem
              disabled={!previewEnabled}
              onSelect={() => {
                if (previewEnabled) onPreview()
              }}
            >
              <Eye className="h-4 w-4" />
              <span>{previewEnabled ? 'Preview inside' : 'Preview unavailable'}</span>
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={!openEnabled}
              onSelect={() => {
                if (openEnabled) openExternalAction?.()
              }}
            >
              <ExternalLink className="h-4 w-4" />
              <span>Open with default app</span>
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={!revealEnabled}
              onSelect={() => {
                if (revealEnabled) onReveal()
              }}
            >
              <FolderOpen className="h-4 w-4" />
              <span>Show in folder</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  )
}
