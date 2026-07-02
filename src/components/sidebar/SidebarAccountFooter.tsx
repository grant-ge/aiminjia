import { useState } from "react";
import type { ReactNode } from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import {
  Check,
  ChevronRight,
  LayoutDashboard,
  Info,
  Languages,
  LogOut,
  Palette,
  Rows3,
  Settings,
  Type,
  Cpu,
  X,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { getSettings, setImChannelKeepAwake, updateSettings } from "@/lib/tauri";
import { tenantHost } from "@/lib/environment";
import { cn } from "@/lib/utils";
import { useAuthStore } from "@/stores/authStore";
import { DEFAULTS, useBrandingStore } from "@/stores/brandingStore";
import { useSettingsStore } from "@/stores/settingsStore";
import type { SettingsModalKey } from "@/stores/uiStore";
import type { AppLanguage } from "@/i18n";
import type { ChatWidthMode, FontScale } from "@/types/settings";

interface SidebarAccountFooterProps {
  onOpenSettings: (key: SettingsModalKey) => void;
  onIdentityClick?: () => void;
}

const PROD_ADMIN_PORTAL_URL = "https://ai-tenant.renlijia.com/members";
const TEST_ADMIN_PORTAL_URL = "https://test-ai-tenant.renlijia.com/members";
type PreferencePanelKey =
  | "language"
  | "fontSize"
  | "chatWidth"
  | "keepAwake";

const preferenceDetailVerticalClass: Record<PreferencePanelKey, string> = {
  language: "top-0",
  fontSize: "bottom-0",
  chatWidth: "top-16",
  keepAwake: "bottom-0",
};

function MenuShell({
  label,
  widthClass,
  children,
}: {
  label: string;
  widthClass: string;
  children: ReactNode;
}) {
  return (
    <div
      role="menu"
      aria-label={label}
      className={cn(
        "rounded-md border border-border bg-popover p-1.5 text-popover-foreground shadow-[var(--shadow-popover)]",
        widthClass,
      )}
    >
      {children}
    </div>
  );
}

function MenuButton({
  label,
  icon: Icon,
  active = false,
  danger = false,
  showChevron = false,
  activateOnHover = false,
  onHover,
  onClick,
  children,
}: {
  label: string;
  icon: LucideIcon;
  active?: boolean;
  danger?: boolean;
  showChevron?: boolean;
  activateOnHover?: boolean;
  onHover?: () => void;
  onClick?: () => void;
  children?: ReactNode;
}) {
  return (
    <Button unstyled
      type="button"
      aria-current={active ? "true" : undefined}
      aria-label={label}
      className={cn(
        "flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-sm font-medium transition-colors",
        active
          ? "bg-accent text-accent-foreground"
          : "text-foreground hover:bg-accent hover:text-accent-foreground",
        danger && "text-destructive hover:bg-[rgba(var(--destructive-rgb),0.10)] hover:text-destructive",
      )}
      onPointerEnter={activateOnHover ? onClick : onHover}
      onMouseEnter={activateOnHover ? onClick : onHover}
      onFocus={activateOnHover ? onClick : onHover}
      onClick={onClick}
    >
      <Icon
        className={cn(
          "h-4 w-4 shrink-0",
          active ? "text-accent-foreground" : "text-muted-foreground",
          danger && "text-destructive",
        )}
        aria-hidden
      />
      <span className="min-w-0 flex-1 truncate">{children ?? label}</span>
      {showChevron ? (
        <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
      ) : null}
    </Button>
  );
}

function ChoiceRow({
  label,
  checked,
  icon: Icon,
  colorClass,
  onClick,
}: {
  label: string;
  checked: boolean;
  icon?: LucideIcon;
  colorClass?: string;
  onClick: () => void;
}) {
  return (
    <Button unstyled
      type="button"
      role="menuitemradio"
      aria-checked={checked}
      aria-label={label}
      className="flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-left text-sm font-medium text-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
      onClick={onClick}
    >
      {Icon ? (
        <Icon className="h-4 w-4 shrink-0 text-foreground" aria-hidden />
      ) : (
        <span
          className={cn(
            "flex h-[14px] w-[14px] shrink-0 items-center justify-center rounded-full border",
            checked ? "border-primary" : "border-border",
            colorClass,
          )}
          aria-hidden
        >
          {checked ? (
            <span
              data-aijia-choice-indicator-dot
              className="h-[6px] w-[6px] rounded-full bg-primary"
            />
          ) : null}
        </span>
      )}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {checked ? <Check className="h-4 w-4 shrink-0 text-foreground" aria-hidden /> : null}
    </Button>
  );
}

function MenuDivider() {
  return <div className="my-1 h-px bg-border" />;
}

export function SidebarAccountFooter({
  onOpenSettings,
  onIdentityClick,
}: SidebarAccountFooterProps) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const [adminPortalOpen, setAdminPortalOpen] = useState(false);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [activePreference, setActivePreference] = useState<PreferencePanelKey | null>(null);
  const user = useAuthStore((s) => s.user);
  const logout = useAuthStore((s) => s.logout);
  const productName = useBrandingStore((s) => s.productName);
  const logoUrl = useBrandingStore((s) => s.logoUrl);
  const fontScale = useSettingsStore((s) => s.fontScale ?? "medium");
  const setFontScale = useSettingsStore((s) => s.setFontScale);
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? "full");
  const setChatWidthMode = useSettingsStore((s) => s.setChatWidthMode);
  const imChannelKeepAwakeEnabled = useSettingsStore((s) =>
    Boolean(s.imChannelKeepAwakeEnabled),
  );
  const setImChannelKeepAwakeEnabled = useSettingsStore(
    (s) => s.setImChannelKeepAwakeEnabled,
  );
  const appLanguage: AppLanguage = i18n.language === "en-US" ? "en-US" : "zh-CN";
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage);

  const displayName =
    user?.name?.trim() || user?.username?.trim() || t("settings.notLoggedIn");
  const accountSubtitle = productName.trim() || t("sidebar.account.productFallback");
  const accountLogoSrc = logoUrl.trim() || DEFAULTS.logoUrl;
  const canOpenAdminPortal = user?.role === "admin";
  const adminPortalUrl = tenantHost().includes("test-ai-tenant")
    ? TEST_ADMIN_PORTAL_URL
    : PROD_ADMIN_PORTAL_URL;

  const persistToBackend = async (patch: {
    fontScale?: FontScale;
    appLanguage?: AppLanguage;
    chatWidthMode?: ChatWidthMode;
    imChannelKeepAwakeEnabled?: boolean;
  }) => {
    try {
      const current = await getSettings();
      await updateSettings({ ...current, ...patch });
    } catch (err) {
      console.error("Failed to persist sidebar quick preferences:", err);
    }
  };

  const openSettingsPage = (key: SettingsModalKey) => {
    setOpen(false);
    setPreferencesOpen(false);
    setActivePreference(null);
    onOpenSettings(key);
  };

  const handleLogout = () => {
    setOpen(false);
    setPreferencesOpen(false);
    setActivePreference(null);
    void logout().catch((err) => {
      console.error("Failed to logout from sidebar account menu:", err);
    });
  };

  const handleOpenAdminPortal = () => {
    setOpen(false);
    setPreferencesOpen(false);
    setActivePreference(null);
    setAdminPortalOpen(true);
  };

  const closePreferencesCascade = () => {
    setPreferencesOpen(false);
    setActivePreference(null);
  };

  const handleFontScaleChange = (value: FontScale) => {
    setFontScale(value);
    void persistToBackend({ fontScale: value });
  };

  const handleLanguageChange = (value: AppLanguage) => {
    setAppLanguage(value);
    void persistToBackend({ appLanguage: value });
  };

  const handleChatWidthModeChange = (value: ChatWidthMode) => {
    setChatWidthMode(value);
    void persistToBackend({ chatWidthMode: value });
  };

  const handleImChannelKeepAwakeChange = (value: "off" | "on") => {
    const enabled = value === "on";
    setImChannelKeepAwakeEnabled(enabled);
    void setImChannelKeepAwake(enabled).catch((err) => {
      console.error("Failed to apply sidebar IM channel keep-awake setting:", err);
    });
    void persistToBackend({ imChannelKeepAwakeEnabled: enabled });
  };

  const mainMenuItems: Array<{
    key: string;
    label: string;
    icon: LucideIcon;
    onClick: () => void;
    showChevron?: boolean;
    danger?: boolean;
  }> = [
    {
      key: "settings",
      label: t("sidebar.account.settings"),
      icon: Settings,
      onClick: () => openSettingsPage("account"),
    },
  ];

  if (canOpenAdminPortal) {
    mainMenuItems.push({
      key: "adminPortal",
      label: t("sidebar.account.adminPortal"),
      icon: LayoutDashboard,
      onClick: handleOpenAdminPortal,
    });
  }

  const preferenceItems: Array<{
    key: PreferencePanelKey;
    label: string;
    icon: LucideIcon;
  }> = [
    { key: "language", label: t("sidebar.account.language"), icon: Languages },
    { key: "fontSize", label: t("sidebar.account.fontSize"), icon: Type },
    { key: "chatWidth", label: t("sidebar.account.chatWidth"), icon: Rows3 },
    { key: "keepAwake", label: t("sidebar.account.imKeepAwake"), icon: Cpu },
  ];

  const activePreferenceLabel =
    preferenceItems.find((item) => item.key === activePreference)?.label ?? "";

  const accountTriggerLabel = `${t("sidebar.account.trigger")} ${displayName} ${accountSubtitle}`;

  const renderPreferenceDetail = () => {
    if (!activePreference) {
      return null;
    }

    if (activePreference === "language") {
      return (
        <>
          <ChoiceRow
            label={t("settings.general.languageZh")}
            checked={appLanguage === "zh-CN"}
            onClick={() => handleLanguageChange("zh-CN")}
          />
          <ChoiceRow
            label={t("settings.general.languageEn")}
            checked={appLanguage === "en-US"}
            onClick={() => handleLanguageChange("en-US")}
          />
        </>
      );
    }

    if (activePreference === "fontSize") {
      return (
        <>
          <ChoiceRow
            label={t("settings.general.fontSmall")}
            checked={fontScale === "small"}
            onClick={() => handleFontScaleChange("small")}
          />
          <ChoiceRow
            label={t("settings.general.fontMedium")}
            checked={fontScale === "medium"}
            onClick={() => handleFontScaleChange("medium")}
          />
          <ChoiceRow
            label={t("settings.general.fontLarge")}
            checked={fontScale === "large"}
            onClick={() => handleFontScaleChange("large")}
          />
        </>
      );
    }

    if (activePreference === "chatWidth") {
      return (
        <>
          <ChoiceRow
            label={t("settings.general.chatWidthCentered")}
            checked={chatWidthMode === "centered"}
            onClick={() => handleChatWidthModeChange("centered")}
          />
          <ChoiceRow
            label={t("settings.general.chatWidthFull")}
            checked={chatWidthMode === "full"}
            onClick={() => handleChatWidthModeChange("full")}
          />
        </>
      );
    }

    return (
      <>
        <ChoiceRow
          label={t("settings.general.switchOff")}
          checked={!imChannelKeepAwakeEnabled}
          onClick={() => handleImChannelKeepAwakeChange("off")}
        />
        <ChoiceRow
          label={t("settings.general.switchOn")}
          checked={imChannelKeepAwakeEnabled}
          onClick={() => handleImChannelKeepAwakeChange("on")}
        />
        <div className="px-2 py-1.5 text-xs leading-5 text-muted-foreground">
          {t("sidebar.account.imKeepAwakeHint", { productName: accountSubtitle })}
        </div>
      </>
    );
  };

  return (
    <div className="px-2 py-2">
      <PopoverPrimitive.Root
        open={open}
        onOpenChange={(nextOpen) => {
          setOpen(nextOpen);
          if (!nextOpen) {
            setPreferencesOpen(false);
            setActivePreference(null);
          }
        }}
      >
        <PopoverPrimitive.Trigger asChild>
          <div
            role="button"
            tabIndex={0}
            aria-label={accountTriggerLabel}
            className="flex h-12 w-full cursor-pointer items-center gap-2 rounded-md text-left text-sidebar-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            style={{ cursor: "pointer" }}
            onClick={onIdentityClick}
            onKeyDown={(event) => {
              if (event.key !== "Enter" && event.key !== " ") return;
              event.preventDefault();
              event.currentTarget.click();
            }}
          >
            <div
              data-testid="sidebar-account-avatar"
              className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md border border-sidebar-border bg-card text-sm font-semibold text-primary"
            >
              <img src={accountLogoSrc} alt="" className="h-full w-full object-cover" />
            </div>

            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold leading-5 text-sidebar-foreground">
                {displayName}
              </div>
              <div className="truncate text-xs leading-4 text-muted-foreground">
                {accountSubtitle}
              </div>
            </div>

            <span
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground"
              aria-hidden
            >
              <Settings className="h-4 w-4" />
            </span>
          </div>
        </PopoverPrimitive.Trigger>
        <PopoverPrimitive.Portal>
          <PopoverPrimitive.Content
            aria-label={t("sidebar.account.trigger")}
            side="top"
            align="start"
            sideOffset={2}
            className="z-50 max-w-[calc(100vw-24px)] overflow-visible bg-transparent text-popover-foreground outline-none"
            onMouseLeave={() => {
              setPreferencesOpen(false);
              setActivePreference(null);
            }}
          >
            <div className="relative [--sidebar-account-detail-menu-width:180px] [--sidebar-account-menu-overlap:0.375rem] [--sidebar-account-menu-width:200px] [--sidebar-account-preferences-menu-width:188px]">
              <MenuShell
                label={t("sidebar.account.trigger")}
                widthClass="[--sidebar-account-menu-width:200px] w-[var(--sidebar-account-menu-width)]"
              >
                {mainMenuItems.map((item) => (
                  <MenuButton
                    key={item.key}
                    label={item.label}
                    icon={item.icon}
                    active={item.key === "preferences"}
                    danger={item.danger}
                    showChevron={item.showChevron}
                    onHover={closePreferencesCascade}
                    onClick={item.onClick}
                  />
                ))}
                <MenuButton
                  label={t("sidebar.account.preferences")}
                  icon={Palette}
                  active={preferencesOpen}
                  showChevron
                  activateOnHover
                  onClick={() => {
                    setPreferencesOpen(true);
                    setActivePreference(null);
                  }}
                />
                <MenuButton
                  label={t("sidebar.account.about")}
                  icon={Info}
                  onHover={closePreferencesCascade}
                  onClick={() => openSettingsPage("about")}
                />
                <MenuDivider />
                <MenuButton
                  label={t("sidebar.account.logout")}
                  icon={LogOut}
                  danger
                  onHover={closePreferencesCascade}
                  onClick={handleLogout}
                />
              </MenuShell>

              {preferencesOpen ? (
                <MenuShell
                  label={t("sidebar.account.preferences")}
                  widthClass="absolute bottom-0 left-[calc(var(--sidebar-account-menu-width)-var(--sidebar-account-menu-overlap))] w-[var(--sidebar-account-preferences-menu-width)]"
                >
                  {preferenceItems.map((item) => (
                    <MenuButton
                      key={item.key}
                      label={item.label}
                      icon={item.icon}
                      active={item.key === activePreference}
                      showChevron
                      activateOnHover
                      onClick={() => setActivePreference(item.key)}
                    />
                  ))}
                </MenuShell>
              ) : null}

              {activePreference ? (
                <MenuShell
                  label={activePreferenceLabel}
                  widthClass={cn(
                    "absolute left-[calc(var(--sidebar-account-menu-width)+var(--sidebar-account-preferences-menu-width)-var(--sidebar-account-menu-overlap)-var(--sidebar-account-menu-overlap))] w-[var(--sidebar-account-detail-menu-width)]",
                    preferenceDetailVerticalClass[activePreference],
                  )}
                >
                  {renderPreferenceDetail()}
                </MenuShell>
              ) : null}
            </div>
          </PopoverPrimitive.Content>
        </PopoverPrimitive.Portal>
      </PopoverPrimitive.Root>
      <Dialog open={adminPortalOpen} onOpenChange={setAdminPortalOpen}>
        <DialogContent
          className="flex h-[90vh] w-[90vw] max-w-none flex-col gap-0 overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]"
          hideClose
        >
          <div className="flex h-12 shrink-0 items-center justify-between border-b border-border bg-card px-5">
            <DialogTitle className="min-w-0 flex-1 truncate text-base leading-6">
              {t("sidebar.account.adminPortal")}
            </DialogTitle>
            <DialogDescription className="sr-only">
              {t("sidebar.account.adminPortalDescription")}
            </DialogDescription>
            <DialogClose
              aria-label={t("common.close")}
              className="-mr-2 flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none"
            >
              <X className="h-4 w-4" aria-hidden />
            </DialogClose>
          </div>
          <iframe
            title={t("sidebar.account.adminPortal")}
            src={adminPortalUrl}
            className="min-h-0 flex-1 border-0 bg-background"
            referrerPolicy="no-referrer"
            allow="clipboard-read; clipboard-write"
          />
        </DialogContent>
      </Dialog>
    </div>
  );
}
