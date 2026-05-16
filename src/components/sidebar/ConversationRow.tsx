/**
 * @designSource design.pen#0EZDr / HsGnf / GknhC
 * @sizing padding [6,8,6,30] (indent 30 under ProjectAccordion), fontSize 13
 */
import { Archive, Copy, Ellipsis, Loader2, Pencil } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { AppDropdown, type AppDropdownItem } from "@/components/common/AppDropdown";

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
  const { t } = useTranslation();
  const [hovered, setHovered] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  const showMore = hovered || menuOpen || active;
  const menuItems: AppDropdownItem[] = [
    {
      id: 'archive',
      label: t('sidebar.archiveChat'),
      icon: <Archive className="h-3.5 w-3.5 shrink-0" />,
      onSelect: () => onArchive?.(),
    },
    {
      id: 'rename',
      label: t('sidebar.renameChat'),
      icon: <Pencil className="h-3.5 w-3.5 shrink-0" />,
      onSelect: () => onRename?.(),
    },
    {
      id: 'copy-id',
      label: t('sidebar.copyConversationId'),
      icon: <Copy className="h-3.5 w-3.5 shrink-0" />,
      onSelect: () => void navigator.clipboard.writeText(id),
    },
  ];

  const wrapperCls = active
    ? "flex items-center rounded-md pl-8 pr-2 bg-sidebar-accent text-sidebar-foreground"
    : "flex items-center rounded-md pl-8 pr-2 text-sidebar-foreground/70 transition-colors hover:bg-sidebar-accent/40";

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
          className="group flex flex-1 min-w-0 items-center py-1.5 pr-2 text-left text-sm"
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
          <AppDropdown
            open={menuOpen}
            onOpenChange={setMenuOpen}
            ariaLabel={t('sidebar.chatMoreActions')}
            contentClassName="w-40"
            trigger={
              <button
                type="button"
                onClick={(e) => e.stopPropagation()}
                className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-foreground"
              >
                <Ellipsis className="h-3.5 w-3.5" />
              </button>
            }
            items={menuItems}
          />
        </div>
      </div>
    </div>
  );
}
