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
} from "lucide-react";
import type { ReactNode } from "react";
import { Button } from '@/components/ui/button'
import { ChatAvatar } from '@/components/chat-scene/ChatAvatar'
import { AppDropdown, type AppDropdownItem } from '@/components/common/AppDropdown'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip'
import { handleChromeDragRegionMouseDown } from '@/components/layout/windowChrome'
import { cn } from '@/lib/utils'
import { useUiStore } from '@/stores/uiStore'

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
  workspacePath?: string | null;
  workspaceAvailable?: boolean | null;
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
  onMore?: () => void;
  moreMenuItems?: AppDropdownItem[];
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
  workspacePath,
  workspaceAvailable,
  kind,
  sourceLabel,
  employee,
  expertTeam,
  onMore,
  moreMenuItems,
  onToggleSidebar,
  trailing,
}: ChatTopBarProps) {
  const sidebarHidden = useUiStore((state) => state.sidebarHidden);
  const reserveMacWindowControlInset =
    navigator.userAgent.includes("Macintosh") && sidebarHidden;
  const workspaceMissing = workspaceAvailable === false;
  const workspaceStatus = workspaceMissing
    ? "missing"
    : workspaceAvailable === true
      ? "available"
      : "unknown";
  const workspaceTitle = workspaceMissing
    ? `工作目录不存在：${workspacePath ?? workspace ?? ""}`
    : workspacePath ?? workspace;
  const workspaceChip = workspace ? (
    <span
      data-testid="chat-topbar-workspace"
      data-aijia-workspace-status={workspaceStatus}
      title={workspaceTitle}
      tabIndex={workspaceTitle ? 0 : undefined}
      className={cn(
        "flex items-center gap-1 truncate text-xs",
        workspaceMissing ? "text-destructive" : "text-muted-foreground",
      )}
    >
      <Folder className="h-3 w-3 shrink-0" aria-hidden />
      <span className="truncate">{workspace}</span>
    </span>
  ) : null;

  return (
    <header
      data-tauri-drag-region
      onMouseDown={handleChromeDragRegionMouseDown}
      className={cn(
        "relative z-20 flex h-12 shrink-0 items-center justify-between border-b border-border bg-background px-6 transition-[padding] duration-200 ease-out motion-reduce:transition-none",
        // Reserve the macOS window-controls strip when the sidebar is collapsed.
        reserveMacWindowControlInset && "pl-48",
      )}
    >
      <div className="flex min-w-0 items-center gap-3">
        {employee ? (
          <Button unstyled
            type="button"
            data-testid="chat-topbar-employee"
            onClick={employee.onClick}
            disabled={!employee.onClick}
            className="flex min-w-0 items-center gap-2 rounded px-1.5 py-1 text-sm font-semibold text-foreground transition-colors hover:bg-accent disabled:cursor-default disabled:hover:bg-transparent"
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
        {workspaceChip && workspaceTitle ? (
          <TooltipProvider delayDuration={200}>
            <Tooltip>
              <TooltipTrigger asChild>{workspaceChip}</TooltipTrigger>
              <TooltipContent side="bottom" className="max-w-96 whitespace-normal break-all">
                {workspaceTitle}
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : workspaceChip}
        {kind && kind !== "user" && kind !== "employee" && !expertTeam && sourceLabel ? (
          <>
            <span aria-hidden className="text-xs text-[rgba(var(--muted-foreground-rgb),0.40)]">
              ·
            </span>
            <SourceChip kind={kind} label={sourceLabel} />
          </>
        ) : null}
      </div>
      <div className="flex items-center gap-1.5">
        {trailing}
        {moreMenuItems && moreMenuItems.length > 0 ? (
          <AppDropdown
            ariaLabel="更多"
            align="end"
            sideOffset={6}
            items={moreMenuItems}
            trigger={
              <Button unstyled
                type="button"
                title="更多"
                className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              >
                <Ellipsis className="h-4 w-4" />
              </Button>
            }
          />
        ) : onMore ? (
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
