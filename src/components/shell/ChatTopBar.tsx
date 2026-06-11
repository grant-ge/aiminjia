/**
 * @designSource design.pen#qLmzZ
 * @sizing height 48, padding [0,24], bottom border 1, left gap 12, right gap 14
 */
import {
  Ellipsis,
  Folder,
  GraduationCap,
  MessageSquare,
  PanelLeft,
  Share2,
} from "lucide-react";
import type { ReactNode } from "react";
import { Button } from '@/components/ui/button'
import { ChatAvatar } from '@/components/chat-scene/ChatAvatar'

export interface ChatTopBarEmployee {
  avatar: string;
  avatarUrl?: string | null;
  name: string;
  role: string;
  defaultSkillLabel?: string | null;
  onClick?: () => void;
}

export interface ChatTopBarExpertTeam {
  avatar: ReactNode;
  name: string;
  tagline?: string;
}

export type ChatTopBarKind = "user" | "employee" | "expertTeam" | "im";

interface ChatTopBarProps {
  title: string;
  workspace?: string;
  /** Conversation kind. Drives the source-label chip (expert team / IM channel). */
  kind?: ChatTopBarKind;
  /** 来源副标题: 员工 display name / 团名 / 渠道名. */
  sourceLabel?: string;
  /** Updated-at ISO string kept for callers; no longer rendered in the top bar. */
  updatedAt?: string;
  /**
   * When set, replaces the plain title with an employee identity card
   * (avatar · name · role). Used for conversations created by employee
   * dispatch. Click handler typically opens the employee drawer.
   */
  employee?: ChatTopBarEmployee;
  /** Expert-team identity chip, matching the employee top-bar treatment. */
  expertTeam?: ChatTopBarExpertTeam;
  onShare?: () => void;
  shareLabel?: string;
  onMore?: () => void;
  onToggleSidebar?: () => void;
  /** extra node rendered at the right edge */
  trailing?: ReactNode;
}

/** Source-label chip — kind-specific icon plus the human-readable label. */
function SourceChip({ kind, label }: { kind: ChatTopBarKind; label: string }) {
  const Icon =
    kind === "expertTeam"
      ? GraduationCap
      : kind === "im"
        ? MessageSquare
        : null;
  if (!Icon) return null;
  return (
    <span className="flex items-center gap-1 truncate text-xs text-muted-foreground">
      <Icon className="h-3 w-3 shrink-0" aria-hidden />
      <span className="truncate">{label}</span>
    </span>
  );
}

export function ChatTopBar({
  title,
  workspace,
  kind,
  sourceLabel,
  employee,
  expertTeam,
  onShare,
  shareLabel = "分享",
  onMore,
  onToggleSidebar,
  trailing,
}: ChatTopBarProps) {
  return (
    <header
      data-tauri-drag-region
      className="flex h-12 shrink-0 items-center justify-between border-b border-border bg-background px-6"
    >
      <div className="flex min-w-0 items-center gap-3">
        {employee ? (
          <Button unstyled
            type="button"
            data-testid="chat-topbar-employee"
            onClick={employee.onClick}
            disabled={!employee.onClick}
            className="flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-sm font-semibold text-foreground transition-colors hover:bg-accent disabled:cursor-default disabled:hover:bg-transparent"
          >
            <ChatAvatar
              name={employee.name}
              src={employee.avatarUrl}
              size={30}
              variant="neutral"
            />
            <span className="min-w-0 truncate">
              {employee.role ? `${employee.role} · ${employee.name}` : employee.name}
            </span>
            {employee.defaultSkillLabel ? (
              <span className="ml-1 max-w-[220px] shrink-0 truncate rounded-[2px] bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground">
                {employee.defaultSkillLabel}
              </span>
            ) : null}
          </Button>
        ) : expertTeam ? (
          <div
            className="flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-sm font-semibold text-foreground"
            data-testid="chat-topbar-expert-team"
          >
            {expertTeam.avatar}
            <span className="min-w-0 truncate">
              专家团 · {expertTeam.name}
            </span>
            {expertTeam.tagline ? (
              <span className="ml-1 max-w-[220px] shrink-0 truncate text-xs font-medium text-muted-foreground">
                {expertTeam.tagline}
              </span>
            ) : null}
          </div>
        ) : (
          <div className="truncate text-sm font-semibold leading-[22px] tracking-normal text-foreground">
            {title}
          </div>
        )}
        {/* Meta chips — workspace / source. Each chip
            owns a leading separator so the visual rhythm stays consistent
            even when individual chips are missing. */}
        {workspace ? (
          <span className="flex items-center gap-1 truncate text-xs text-muted-foreground">
            <Folder className="h-3 w-3 shrink-0" aria-hidden />
            <span className="truncate">{workspace}</span>
          </span>
        ) : null}
        {kind && kind !== "user" && kind !== "employee" && !expertTeam && sourceLabel ? (
          <>
            <span aria-hidden className="text-xs text-muted-foreground/40">
              ·
            </span>
            <SourceChip kind={kind} label={sourceLabel} />
          </>
        ) : null}
      </div>
      <div className="flex items-center gap-1.5">
        {trailing}
        {onShare ? (
          <Button unstyled
            type="button"
            aria-label={shareLabel}
            title={shareLabel}
            onClick={onShare}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Share2 className="h-4 w-4" />
          </Button>
        ) : null}
        {onMore ? (
          <Button unstyled
            type="button"
            aria-label="更多"
            onClick={onMore}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Ellipsis className="h-4 w-4" />
          </Button>
        ) : null}
        {onToggleSidebar ? (
          <Button unstyled
            type="button"
            aria-label="折叠侧栏"
            onClick={onToggleSidebar}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <PanelLeft className="h-4 w-4" />
          </Button>
        ) : null}
      </div>
    </header>
  );
}
