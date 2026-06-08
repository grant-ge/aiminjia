/**
 * @designSource design.pen#0EZDr / HsGnf / GknhC
 * @sizing padding [6,8,6,30] (indent 30 under ProjectAccordion), fontSize 13
 */
import { Archive, Copy, Loader2, Pencil, Pin, PinOff } from 'lucide-react'
import * as ContextMenuPrimitive from '@radix-ui/react-context-menu'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

interface ConversationRowProps {
  id: string
  title: string
  active?: boolean
  indent?: boolean
  loading?: boolean
  pinned?: boolean
  onClick: () => void
  onArchive?: () => void
  onRename?: () => void
  onTogglePin?: () => void
}

const CONFIRM_RESET_MS = 3000

export function ConversationRow({
  id,
  title,
  active = false,
  indent = true,
  loading = false,
  pinned = false,
  onClick,
  onArchive,
  onRename,
  onTogglePin,
}: ConversationRowProps) {
  const { t } = useTranslation()
  const [hovered, setHovered] = useState(false)
  const [armed, setArmed] = useState(false)
  const armedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // The archive icon button is only revealed on hover or for the active row.
  // The active row keeps it visible so the user can find it without first
  // moving the cursor onto the (already-selected) row.
  const showArchive = hovered || active

  useEffect(() => {
    return () => {
      if (armedTimerRef.current) clearTimeout(armedTimerRef.current)
    }
  }, [])

  const disarm = () => {
    if (armedTimerRef.current) {
      clearTimeout(armedTimerRef.current)
      armedTimerRef.current = null
    }
    setArmed(false)
  }

  const handleArchiveClick = (e: React.MouseEvent) => {
    e.stopPropagation()
    if (!onArchive) return
    if (armed) {
      // Second click within the armed window — actually archive.
      disarm()
      onArchive()
      return
    }
    setArmed(true)
    if (armedTimerRef.current) clearTimeout(armedTimerRef.current)
    armedTimerRef.current = setTimeout(() => setArmed(false), CONFIRM_RESET_MS)
  }

  const paddingCls = indent ? 'pl-[32px] pr-2' : 'pl-2 pr-2'
  const wrapperCls = active
    ? `flex h-8 items-center rounded-md ${paddingCls} bg-sidebar-accent text-sidebar-foreground`
    : `flex h-8 items-center rounded-md ${paddingCls} text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/60 hover:text-sidebar-foreground`

  return (
    <ContextMenuPrimitive.Root>
      <ContextMenuPrimitive.Trigger asChild>
        <div
          className="pr-1"
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => {
            setHovered(false)
            // Disarm on leave so the red confirm state never persists once the
            // user moves away — armed state should always be user-attended.
            if (armed) disarm()
          }}
        >
          <div className={wrapperCls}>
            <button
              type="button"
              onClick={onClick}
              className="group flex min-w-0 flex-1 items-center pr-2 text-left text-sm"
              data-aijia-conversation-row
              data-aijia-conversation-id={id}
            >
              {loading ? (
                <Loader2
                  data-icon="loader"
                  className="h-3.5 w-3.5 shrink-0 animate-spin text-sidebar-foreground"
                />
              ) : null}
              <span className="truncate">{title}</span>
            </button>

            <div className={showArchive ? 'flex shrink-0 items-center gap-0.5' : 'hidden'}>
              <TooltipProvider delayDuration={400}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      aria-label={
                        pinned
                          ? t('sidebar.unpinChat')
                          : t('sidebar.pinChat')
                      }
                      onClick={(e) => {
                        e.stopPropagation()
                        onTogglePin?.()
                      }}
                      className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-foreground"
                    >
                      {pinned ? (
                        <PinOff className="h-3.5 w-3.5" />
                      ) : (
                        <Pin className="h-3.5 w-3.5" />
                      )}
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">
                    {pinned ? t('sidebar.unpinChat') : t('sidebar.pinChat')}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <TooltipProvider delayDuration={400}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      aria-label={t('sidebar.archiveChat')}
                      onClick={handleArchiveClick}
                      className={cn(
                        'flex h-5 items-center justify-center rounded transition-colors',
                        armed
                          ? 'w-auto bg-destructive px-1.5 text-[10px] font-semibold leading-none text-destructive-foreground'
                          : 'w-5 text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-foreground',
                      )}
                    >
                      {armed ? (
                        <span>{t('common.confirm')}</span>
                      ) : (
                        <Archive className="h-3.5 w-3.5" />
                      )}
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">
                    {armed
                      ? t('sidebar.archiveChatConfirmTooltip')
                      : t('sidebar.archiveChatTooltip')}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            </div>
          </div>
        </div>
      </ContextMenuPrimitive.Trigger>
      <ContextMenuPrimitive.Portal>
        <ContextMenuPrimitive.Content
          className="z-50 min-w-[10rem] overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-[var(--shadow-popover)]"
        >
          <ContextMenuPrimitive.Item
            onSelect={() => onTogglePin?.()}
            className="flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
          >
            {pinned ? (
              <PinOff className="h-3.5 w-3.5 shrink-0" />
            ) : (
              <Pin className="h-3.5 w-3.5 shrink-0" />
            )}
            <span>{pinned ? t('sidebar.unpinChat') : t('sidebar.pinChat')}</span>
          </ContextMenuPrimitive.Item>
          <ContextMenuPrimitive.Item
            onSelect={() => onArchive?.()}
            className="flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
          >
            <Archive className="h-3.5 w-3.5 shrink-0" />
            <span>{t('sidebar.archiveChat')}</span>
          </ContextMenuPrimitive.Item>
          <ContextMenuPrimitive.Item
            onSelect={() => onRename?.()}
            className="flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
          >
            <Pencil className="h-3.5 w-3.5 shrink-0" />
            <span>{t('sidebar.renameChat')}</span>
          </ContextMenuPrimitive.Item>
          <ContextMenuPrimitive.Item
            onSelect={() => void navigator.clipboard.writeText(id)}
            className="flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
          >
            <Copy className="h-3.5 w-3.5 shrink-0" />
            <span>{t('sidebar.copyConversationId')}</span>
          </ContextMenuPrimitive.Item>
        </ContextMenuPrimitive.Content>
      </ContextMenuPrimitive.Portal>
    </ContextMenuPrimitive.Root>
  )
}
