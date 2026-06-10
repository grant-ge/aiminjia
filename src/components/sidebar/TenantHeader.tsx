/**
 * @designSource design.pen#6xhgh
 * @sizing width fluid, padding 8, gap 8
 */

import type { KeyboardEvent } from "react";

interface TenantHeaderProps {
  name?: string;
  logoUrl?: string;
  onClick?: () => void;
}

export function TenantHeader({
  name = "",
  logoUrl = "/brand-avatar-gold.svg",
  onClick,
}: TenantHeaderProps) {
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    onClick?.();
  };

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onClick}
      onKeyDown={handleKeyDown}
      className="my-2 flex w-full items-center justify-between gap-2 rounded-md px-2.5 text-left transition-colors"
    >
      <div className="flex min-w-0 items-center gap-2">
        <div
          data-testid="tenant-logo"
          className="h-7 w-7 shrink-0 overflow-hidden rounded-md border border-sidebar-border bg-card"
        >
          <img
            src={logoUrl}
            alt="Brand logo"
            className="h-full w-full object-cover"
          />
        </div>
        <div
          data-aijia-product-name
          className="min-w-0 truncate text-sm font-semibold text-sidebar-foreground"
        >
          {name}
        </div>
      </div>
      {/* <ChevronsUpDown
        data-icon="chevrons-up-down"
        className="h-4 w-4 shrink-0 text-muted-foreground"
      /> */}
    </div>
  );
}
