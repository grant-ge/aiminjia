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

export interface ChatTopBarEmployee {
  avatar: string;
  name: string;
  role: string;
  onClick?: () => void;
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
          <button
            type="button"
            data-testid="chat-topbar-employee"
            onClick={employee.onClick}
            disabled={!employee.onClick}
            className="flex min-w-0 items-center gap-1.5 rounded-md px-2 py-1 text-[15px] font-semibold text-foreground transition-colors hover:bg-accent disabled:cursor-default disabled:hover:bg-transparent"
          >
            <span aria-hidden className="text-base leading-none">
              {employee.avatar}
            </span>
            <span className="truncate">{employee.name}</span>
            {employee.role ? (
              <>
                <span aria-hidden className="text-sm text-muted-foreground">
                  ·
                </span>
                <span className="truncate text-sm font-normal text-muted-foreground">
                  {employee.role}
                </span>
              </>
            ) : null}
          </button>
        ) : (
          <div className="truncate text-[15px] font-semibold leading-[22px] tracking-normal text-foreground">
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
        {kind && kind !== "user" && kind !== "employee" && sourceLabel ? (
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
          <button
            type="button"
            aria-label={shareLabel}
            title={shareLabel}
            onClick={onShare}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Share2 className="h-4 w-4" />
          </button>
        ) : null}
        {onMore ? (
          <button
            type="button"
            aria-label="更多"
            onClick={onMore}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <Ellipsis className="h-4 w-4" />
          </button>
        ) : null}
        {onToggleSidebar ? (
          <button
            type="button"
            aria-label="折叠侧栏"
            onClick={onToggleSidebar}
            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          >
            <PanelLeft className="h-4 w-4" />
          </button>
        ) : null}
      </div>
    </header>
  );
}
