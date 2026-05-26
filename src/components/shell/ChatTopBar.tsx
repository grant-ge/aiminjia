/**
 * @designSource design.pen#qLmzZ
 * @sizing height 56, padding [0,24], bottom border 1, left gap 12, right gap 14
 */
import { Ellipsis, Folder, GraduationCap, MessageSquare, PanelLeft, Share2 } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { formatRelativeTime } from '@/lib/format'
import { NetworkStatusIndicator } from './NetworkStatusIndicator'

export interface ChatTopBarEmployee {
  avatar: string
  name: string
  role: string
  onClick?: () => void
}

export type ChatTopBarKind = 'user' | 'employee' | 'expertTeam' | 'im'

interface ChatTopBarProps {
  title: string
  workspace?: string
  /** Conversation kind. Drives the source-label chip (expert team / IM channel). */
  kind?: ChatTopBarKind
  /** 来源副标题: 员工 display name / 团名 / 渠道名. */
  sourceLabel?: string
  /** Updated-at ISO string. Renders a small relative-time chip ("4 天前"). */
  updatedAt?: string
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

/** Source-label chip — kind-specific icon plus the human-readable label. */
function SourceChip({ kind, label }: { kind: ChatTopBarKind; label: string }) {
  const Icon =
    kind === 'expertTeam' ? GraduationCap : kind === 'im' ? MessageSquare : null
  if (!Icon) return null
  return (
    <span className="flex items-center gap-1 truncate text-xs text-muted-foreground">
      <Icon className="h-3 w-3 shrink-0" aria-hidden />
      <span className="truncate">{label}</span>
    </span>
  )
}

export function ChatTopBar({
  title,
  workspace,
  kind,
  sourceLabel,
  updatedAt,
  employee,
  onShare,
  onMore,
  onToggleSidebar,
  trailing,
}: ChatTopBarProps) {
  const { t } = useTranslation()
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
          <div className="truncate text-base font-semibold tracking-tight text-foreground">
            {title}
          </div>
        )}
        {/* Meta chips — workspace / source / pinned / updatedAt. Each chip
            owns a leading separator so the visual rhythm stays consistent
            even when individual chips are missing. */}
        {workspace ? (
          <span className="flex items-center gap-1 truncate text-xs text-muted-foreground">
            <Folder className="h-3 w-3 shrink-0" aria-hidden />
            <span className="truncate">{workspace}</span>
          </span>
        ) : null}
        {kind && kind !== 'user' && kind !== 'employee' && sourceLabel ? (
          <>
            <span aria-hidden className="text-xs text-muted-foreground/40">·</span>
            <SourceChip kind={kind} label={sourceLabel} />
          </>
        ) : null}
        {updatedAt ? (
          <>
            <span aria-hidden className="text-xs text-muted-foreground/40">·</span>
            <span className="truncate text-xs text-muted-foreground" title={updatedAt}>
              {t('chatTopBar.updatedAt', { time: formatRelativeTime(updatedAt) })}
            </span>
          </>
        ) : null}
      </div>
      <div className="flex items-center gap-4">
        <NetworkStatusIndicator />
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
