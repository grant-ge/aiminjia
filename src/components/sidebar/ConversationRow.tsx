/**
 * @designSource design.pen#0EZDr / HsGnf / GknhC
 * @sizing padding [6,8,6,30] (indent 30 under ProjectAccordion), fontSize 13
 */
import { Archive, Copy, Ellipsis, Loader2, Pencil, Pin } from "lucide-react";
import { useState } from "react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface ConversationRowProps {
  id: string;
  title: string;
  active?: boolean;
  loading?: boolean;
  onClick: () => void;
  onArchive?: () => void;
  onRename?: () => void;
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
  const [hovered, setHovered] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  const showMore = hovered || menuOpen || active;

  const wrapperCls = active
    ? "flex items-center rounded-md pl-[32px] pr-2 bg-sidebar-accent text-sidebar-foreground"
    : "flex items-center rounded-md pl-[32px] pr-2 text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40";

  return (
    <div
      className="pr-1"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div className={wrapperCls}>
        <button
          type="button"
          onClick={onClick}
          className="group flex flex-1 min-w-0 items-center py-1.5 pr-2 text-left text-[13px]"
        >
          {loading ? (
            <Loader2
              data-icon="loader"
              className="h-3.5 w-3.5 shrink-0 animate-spin text-sidebar-foreground"
            />
          ) : null}
          <span className="truncate">{title}</span>
        </button>

        <div className={showMore ? "block shrink-0" : "hidden"}>
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
            <DropdownMenuContent
              align="end"
              className="w-40 border-sidebar-border bg-sidebar p-1 text-sidebar-foreground [&_[data-highlighted]]:bg-sidebar-accent [&_[data-highlighted]]:text-sidebar-foreground"
            >
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
      </div>
    </div>
  );
}
