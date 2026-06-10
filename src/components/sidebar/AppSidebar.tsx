/**
 * @designSource design.pen#PV1ln (Sidebar) + #EbnTy (Sidebar Content)
 * @sizing width 256, padding 8, gap 16
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CheckSquare,
  Copy,
  GraduationCap,
  MessageSquare,
  Users,
} from "lucide-react";
import * as ContextMenuPrimitive from "@radix-ui/react-context-menu";

import { useChat } from "@/hooks/useChat";
import { useBrandingStore } from "@/stores/brandingStore";
import { useChatStore } from "@/stores/chatStore";
import {
  useUiStore,
  type Route,
  type SidebarBodyTab,
  useActiveConversationId,
  useActiveChannelSessionId,
} from "@/stores/uiStore";
import { useChannelStore } from "@/stores/channelStore";
import { useInteractionStore } from "@/stores/interactionStore";
import { hasExpertTeam } from "@/features/expert-teams/expertTeamRegistry";
import { selectPendingActionForSession } from "@/components/chat-scene/pendingActionSelectors";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

import { ConversationRow } from "./ConversationRow";
import { ConversationTree } from "./ConversationTree";
import { groupConversationsByProject } from "./conversationProjects";
import { DevControlPanel } from "./DevControlPanel";
import {
  SidebarRowStatusIndicator,
  type SidebarRowStatus,
} from "./SidebarRowStatusIndicator";
import { SidebarFooterSettings } from "./SidebarFooterSettings";
import { SidebarNav, type SidebarNavKey } from "./SidebarNav";
import { TenantHeader } from "./TenantHeader";
import type { ChannelConversation } from "@/lib/tauri";

function channelConversationLabel(
  conversation: {
    platform: string;
    conversationType: "group" | "private";
    displayName?: string | null;
  },
  t: (key: string) => string,
): string {
  const platform = t(`channel.platforms.${conversation.platform}.name`);
  const kind =
    conversation.conversationType === "group"
      ? t("channel.chat.groupChat")
      : t("channel.chat.privateChat");
  if (conversation.platform === "whatsapp") {
    const trimmed = conversation.displayName?.trim();
    if (trimmed) return trimmed;
  }
  return `${platform}${kind}`;
}

interface ChannelConversationRowProps {
  active: boolean;
  conversation: ChannelConversation;
  label: string;
  copyLabel: string;
  status?: SidebarRowStatus;
  onSelect: () => void;
}

function ChannelConversationRow({
  active,
  conversation,
  label,
  copyLabel,
  status = null,
  onSelect,
}: ChannelConversationRowProps) {
  const rowClassName = active
    ? "flex h-8 w-full items-center justify-between rounded-md bg-sidebar-accent px-2.5 text-left text-sm font-medium text-sidebar-foreground"
    : "flex h-8 w-full items-center justify-between rounded-md px-2.5 text-left text-sm font-medium text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground";

  return (
    <ContextMenuPrimitive.Root>
      <ContextMenuPrimitive.Trigger asChild>
        <button
          type="button"
          onClick={onSelect}
          className={rowClassName}
          data-aijia-channel-conversation-row
          data-aijia-conversation-id={conversation.sessionId}
        >
          <span className="truncate">{label}</span>
          <span className="ml-2 flex min-w-[44px] shrink-0 items-center justify-end">
            {status ? (
              <SidebarRowStatusIndicator status={status} />
            ) : conversation.unreadCount > 0 ? (
              <span className="rounded-md bg-primary px-1.5 text-xs text-primary-foreground">
                {conversation.unreadCount}
              </span>
            ) : null}
          </span>
        </button>
      </ContextMenuPrimitive.Trigger>
      <ContextMenuPrimitive.Portal>
        <ContextMenuPrimitive.Content className="z-50 min-w-[10rem] overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-[var(--shadow-popover)]">
          <ContextMenuPrimitive.Item
            onSelect={() =>
              void navigator.clipboard.writeText(conversation.sessionId)
            }
            className="flex cursor-default select-none items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
          >
            <Copy className="h-3.5 w-3.5 shrink-0" />
            <span>{copyLabel}</span>
          </ContextMenuPrimitive.Item>
        </ContextMenuPrimitive.Content>
      </ContextMenuPrimitive.Portal>
    </ContextMenuPrimitive.Root>
  );
}

export function AppSidebar() {
  const { t } = useTranslation();
  const productName = useBrandingStore((s) => s.productName);
  const logoUrl = useBrandingStore((s) => s.logoUrl);
  const isWindows =
    typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
  const route = useUiStore((s) => s.route);
  const setRoute = useUiStore((s) => s.setRoute);
  const openSettings = useUiStore((s) => s.openSettings);
  const sidebarTab = useUiStore((s) => s.sidebarTab);
  const setSidebarTab = useUiStore((s) => s.setSidebarTab);
  const {
    conversations,
    switchConversation,
    renameConversation,
    archiveConversation,
    setConversationPinned,
  } = useChat();
  const activeConversationId = useActiveConversationId();
  const channelActiveSessionId = useActiveChannelSessionId();
  const channelConversations = useChannelStore((s) => s.conversations);
  const dingtalkState = useChannelStore((s) => s.platforms.dingtalk);
  const feishuState = useChannelStore((s) => s.platforms.feishu);
  const wecomState = useChannelStore((s) => s.platforms.wecom);
  const wechatState = useChannelStore((s) => s.platforms.wechat);
  const telegramState = useChannelStore((s) => s.platforms.telegram);
  const whatsappState = useChannelStore((s) => s.platforms.whatsapp);
  const busyConversations = useChatStore((s) => s.busyConversations);
  const streamStates = useChatStore((s) => s.streamStates);
  const pendingAsks = useChatStore((s) => s.pendingAsks);
  const pendingInteractions = useInteractionStore((s) => s.pendingInteractions);

  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [devPanelOpen, setDevPanelOpen] = useState(false);
  const [, setTenantHeaderClickCount] = useState(0);

  const switchTab = (next: SidebarBodyTab) => {
    setSidebarTab(next);
  };

  // isActiveRobot=undefined (old data) treated as active for backward compatibility
  const dingtalkConversations = channelConversations.filter(
    (c) => c.platform === "dingtalk",
  );
  const feishuConversations = channelConversations.filter(
    (c) => c.platform === "feishu",
  );
  const wecomConversations = channelConversations.filter(
    (c) => c.platform === "wecom",
  );
  const wechatConversations = channelConversations.filter(
    (c) => c.platform === "wechat",
  );
  const telegramConversations = channelConversations.filter(
    (c) => c.platform === "telegram",
  );
  const whatsappConversations = channelConversations.filter(
    (c) => c.platform === "whatsapp",
  );
  const activeConversations = dingtalkConversations.filter(
    (c) => c.isActiveRobot !== false,
  );
  const activeFeishuConversations = feishuConversations.filter(
    (c) => c.isActiveRobot !== false,
  );
  const activeWecomConversations = wecomConversations.filter(
    (c) => c.isActiveRobot !== false,
  );
  const activeWechatConversations = wechatConversations.filter(
    (c) => c.isActiveRobot !== false,
  );
  const activeTelegramConversations = telegramConversations.filter(
    (c) => c.isActiveRobot !== false,
  );
  const activeWhatsappConversations = whatsappConversations.filter(
    (c) => c.isActiveRobot !== false,
  );

  const handleRenameOpen = (id: string) => {
    const conv = conversations.find((c) => c.id === id);
    setRenameValue(conv?.title ?? "");
    setRenamingId(id);
  };

  const handleRenameConfirm = async () => {
    if (!renamingId || !renameValue.trim()) return;
    await renameConversation(renamingId, renameValue.trim());
    setRenamingId(null);
  };

  const handleArchive = async (id: string) => {
    await archiveConversation(id);
  };

  // IM 频道（钉钉私聊/群）的 session id 复用 conv_store 的 conversation id，
  // 所以会同时出现在 useChat().conversations 里。useChat 已用 kind !== 'im'
  // 过了一遍，这里再用 channelSessionIdSet 做兜底以防 backend kind 标记漏标。
  const channelSessionIdSet = new Set(
    channelConversations.map((c) => c.sessionId),
  );
  const nonChannelConversations = conversations.filter(
    (c) => !channelSessionIdSet.has(c.id),
  );
  // 员工 / 专家团 tab 走独立列表渲染；项目 tab 走白名单：只展示 `kind=user`
  // 或 `kind` 未标记的旧会话（视作 user 兼容）。员工 / 专家团 / IM 类的会话
  // 都被显式排除，避免未来加 kind 时项目 tab 误带入新类型。
  const expertTeamConversations = nonChannelConversations.filter((c) =>
    hasExpertTeam(c.id),
  );
  const employeeConversations = nonChannelConversations.filter(
    (c) => c.kind === "employee",
  );
  const projectConversations = nonChannelConversations.filter(
    (c) => (c.kind ?? "user") === "user" && !hasExpertTeam(c.id),
  );

  // Global pinned section: every pinned in-app conversation regardless of
  // tab kind. Order = useChat ordering (already pinned-first from the Rust
  // sort), so users see consistent ordering whichever tab is active. IM
  // channel sessions are not yet pin-aware (no isPinned column on
  // ChannelConversation) so they're excluded.
  const globalPinned = nonChannelConversations.filter((c) => c.isPinned);

  const isConversationBusy = (conversationId: string) =>
    busyConversations.has(conversationId) ||
    streamStates[conversationId]?.isStreaming === true;

  const sidebarStatusForConversation = (
    conversationId: string,
  ): SidebarRowStatus => {
    const action = selectPendingActionForSession({
      sessionId: conversationId,
      pendingAsks,
      pendingInteractions,
      turnStage: streamStates[conversationId]?.turnStage ?? null,
    });

    if (action?.kind === "permission" || action?.kind === "stale-permission") {
      return "permission-review";
    }

    if (
      action?.kind === "user-question" ||
      action?.kind === "stale-interaction"
    ) {
      return "waiting-reply";
    }

    if (isConversationBusy(conversationId)) {
      return "loading";
    }

    return null;
  };

  const withSidebarState = <T extends { id: string }>(conversation: T) => ({
    ...conversation,
    status: sidebarStatusForConversation(conversation.id),
  });

  // 各 tab 内已经经过全局置顶过滤，避免 pinned 会话同时出现在置顶区和 tab 内。
  const projects = groupConversationsByProject(
    projectConversations.filter((c) => !c.isPinned).map(withSidebarState),
    activeConversationId,
  );

  /**
   * Render a flat tab body (employee / expert-team).  The global pinned
   * section above already surfaces pinned items, so within the tab we just
   * list non-pinned conversations to avoid duplicates.
   */
  const renderFlatTab = (items: typeof conversations) => {
    if (items.length === 0) {
      return (
        <div className="px-2 py-4 text-sm text-muted-foreground">
          {t("sidebar.noHistory")}
        </div>
      );
    }
    const visible = items.filter((c) => !c.isPinned);
    if (visible.length === 0) {
      return (
        <div className="px-2 py-4 text-sm text-muted-foreground">
          {t("sidebar.allPinnedHint")}
        </div>
      );
    }
    return (
      <div className="flex flex-col gap-0.5">
        {visible.map((conversation) => (
          <ConversationRow
            key={conversation.id}
            id={conversation.id}
            title={conversation.title}
            active={activeConversationId === conversation.id}
            indent={false}
            status={sidebarStatusForConversation(conversation.id)}
            pinned={conversation.isPinned ?? false}
            onClick={() => void switchConversation(conversation.id)}
            onRename={() => handleRenameOpen(conversation.id)}
            onArchive={() => void handleArchive(conversation.id)}
            onTogglePin={() =>
              void setConversationPinned(
                conversation.id,
                !(conversation.isPinned ?? false),
              )
            }
          />
        ))}
      </div>
    );
  };

  const activeKey: SidebarNavKey | null =
    route.kind === "channel"
      ? route.sessionId
        ? null
        : "channel"
      : route.kind === "employees"
        ? "employees"
        : route.kind === "skill-center" || route.kind === "skill-detail"
          ? "skill-center"
          : route.kind === "schedules"
            ? "schedules"
            : route.kind === "expert-teams"
              ? "expert-teams"
              : route.kind === "home"
                ? "home"
                : null;

  const tenantDisplay = productName;

  const handleTenantHeaderClick = () => {
    setTenantHeaderClickCount((count) => {
      const next = count + 1;
      if (next >= 7) {
        setDevPanelOpen(true);
        return 0;
      }
      return next;
    });
  };

  function channelStatusLabel(state: typeof dingtalkState): string {
    if (!state?.configured) return t("channel.status.unconfigured");
    if (!state.enabled) return t("channel.status.disconnected");
    switch (state.connection) {
      case "connected":
        return t("channel.status.connected");
      case "connecting":
        return t("channel.status.connecting");
      case "reconnecting":
        return t("channel.status.reconnecting");
      case "configError":
        return t("channel.status.configError");
      case "needsReauth":
        return t("channel.status.needsReauth");
      default:
        return t("channel.status.disconnected");
    }
  }

  function channelEmptyHint(state: typeof dingtalkState): string {
    if (!state?.configured) return t("channel.sidebar.notConfiguredHint");
    if (!state.enabled) return t("channel.sidebar.disabledHint");
    switch (state.connection) {
      case "connected":
        return t("channel.sidebar.noConversations");
      case "connecting":
        return t("channel.sidebar.connectingHint");
      case "needsReauth":
        return t("channel.sidebar.needsReauthHint");
      default:
        return t("channel.sidebar.disconnectedHint");
    }
  }

  const openChannelOverview = () => {
    setRoute({ kind: "channel" });
  };

  const selectChannelSession = (sessionId: string) => {
    setRoute({ kind: "channel", sessionId });
  };

  const renderChannelRows = (items: ChannelConversation[]) =>
    items.map((conversation) => (
      <ChannelConversationRow
        key={conversation.sessionId}
        active={channelActiveSessionId === conversation.sessionId}
        conversation={conversation}
        label={channelConversationLabel(conversation, t)}
        copyLabel={t("sidebar.copyConversationId")}
        status={sidebarStatusForConversation(conversation.sessionId)}
        onSelect={() => selectChannelSession(conversation.sessionId)}
      />
    ));

  return (
    <>
      <aside className="flex h-full w-[256px] shrink-0 flex-col overflow-hidden bg-sidebar px-2 text-sidebar-foreground">
        {isWindows ? null : (
          <TenantHeader
            name={tenantDisplay}
            logoUrl={logoUrl}
            onClick={handleTenantHeaderClick}
          />
        )}

        <SidebarNav
          activeKey={activeKey}
          onSelect={(key) => setRoute({ kind: key } as Route)}
        />

        <div className="flex min-h-0 flex-1 flex-col gap-2">
          {globalPinned.length > 0 ? (
            <div className="flex flex-col mb-1">
              <div className="px-2 py-1 text-xs font-medium text-muted-foreground">
                {t("sidebar.pinnedSection")}
              </div>
              <div className="flex flex-col gap-0.5">
                {globalPinned.map((conversation) => (
                  <ConversationRow
                    key={conversation.id}
                    id={conversation.id}
                    title={conversation.title}
                    active={activeConversationId === conversation.id}
                    indent={false}
                    status={sidebarStatusForConversation(conversation.id)}
                    pinned
                    onClick={() => void switchConversation(conversation.id)}
                    onRename={() => handleRenameOpen(conversation.id)}
                    onArchive={() => void handleArchive(conversation.id)}
                    onTogglePin={() =>
                      void setConversationPinned(conversation.id, false)
                    }
                  />
                ))}
              </div>
            </div>
          ) : null}
          {(() => {
            const TABS = [
              {
                key: "project" as SidebarBodyTab,
                Icon: CheckSquare,
                labelKey: "sidebar.project",
              },
              {
                key: "employee" as SidebarBodyTab,
                Icon: Users,
                labelKey: "sidebar.employeeTab",
              },
              {
                key: "expert-team" as SidebarBodyTab,
                Icon: GraduationCap,
                labelKey: "sidebar.expertTeamTab",
              },
              {
                key: "channel" as SidebarBodyTab,
                Icon: MessageSquare,
                labelKey: "sidebar.channel",
              },
            ];
            const activeIndex = TABS.findIndex((tab) => tab.key === sidebarTab);
            return (
              <div className="relative grid h-8 grid-cols-4 rounded-md border border-sidebar-border bg-sidebar-accent/70 px-1 py-0.5 text-xs font-medium text-muted-foreground">
                {/* Sliding indicator — left/width account for px-1 (4px) horizontal padding */}
                <div
                  className="absolute rounded-md bg-card shadow-sm"
                  style={{
                    top: "2px",
                    bottom: "2px",
                    left: "4px",
                    width: "calc(25% - 2px)",
                    transform: `translateX(${activeIndex * 100}%)`,
                    transition: "transform 200ms ease-in-out",
                  }}
                />
                {TABS.map(({ key, Icon, labelKey }) => (
                  <TooltipProvider key={key} delayDuration={400}>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <button
                          type="button"
                          aria-label={t(labelKey)}
                          onClick={() => switchTab(key)}
                          className={`relative z-10 flex items-center justify-center rounded-md transition-colors duration-200 ${
                            sidebarTab === key ? "text-foreground" : ""
                          }`}
                        >
                          <Icon className="h-3.5 w-3.5 shrink-0" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent side="bottom">
                        {t(labelKey)}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                ))}
              </div>
            );
          })()}

          {sidebarTab === "project" ? (
            <div className="-mr-2 flex-1 overflow-auto">
              <ConversationTree
                projects={projects}
                onSelectConversation={(id) => void switchConversation(id)}
                onRenameConversation={handleRenameOpen}
                onArchiveConversation={(id) => void handleArchive(id)}
                onTogglePinConversation={(id, next) =>
                  void setConversationPinned(id, next)
                }
              />
            </div>
          ) : sidebarTab === "employee" ? (
            <div className="-mr-2 flex-1 overflow-auto py-1">
              {renderFlatTab(employeeConversations)}
            </div>
          ) : sidebarTab === "expert-team" ? (
            <div className="-mr-2 flex-1 overflow-auto py-1">
              {renderFlatTab(expertTeamConversations)}
            </div>
          ) : (
            <div className="-mr-2 flex-1 overflow-auto pr-2">
              <div className="mt-2 flex flex-col gap-3">
                <div>
                  <div className="mb-1.5 flex items-center gap-2 px-2 text-sm font-medium text-sidebar-foreground">
                    <img
                      src="/logos/dingtalk.png"
                      alt=""
                      className="h-5 w-5 rounded-md"
                      draggable={false}
                    />
                    {t("channel.platforms.dingtalk.name")}
                    <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {channelStatusLabel(dingtalkState)}
                    </span>
                  </div>
                  <div className="pl-6">
                    {activeConversations.length === 0 ? (
                      <button
                        type="button"
                        onClick={openChannelOverview}
                        className="w-full rounded-md px-2.5 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                      >
                        {channelEmptyHint(dingtalkState)}
                      </button>
                    ) : (
                      renderChannelRows(activeConversations)
                    )}
                  </div>
                </div>

                <div>
                  <div className="mb-1.5 flex items-center gap-2 px-2 text-sm font-medium text-sidebar-foreground">
                    <img
                      src="/logos/feishu.png"
                      alt=""
                      className="h-5 w-5 rounded-md"
                      draggable={false}
                    />
                    {t("channel.platforms.feishu.name")}
                    <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {channelStatusLabel(feishuState)}
                    </span>
                  </div>
                  <div className="pl-6">
                    {activeFeishuConversations.length === 0 ? (
                      <button
                        type="button"
                        onClick={openChannelOverview}
                        className="w-full rounded-md px-2.5 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                      >
                        {channelEmptyHint(feishuState)}
                      </button>
                    ) : (
                      renderChannelRows(activeFeishuConversations)
                    )}
                  </div>
                </div>

                <div>
                  <div className="mb-1.5 flex items-center gap-2 px-2 text-sm font-medium text-sidebar-foreground">
                    <img
                      src="/logos/wecom.png"
                      alt=""
                      className="h-5 w-5 rounded-md"
                      draggable={false}
                    />
                    {t("channel.platforms.wecom.name")}
                    <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {channelStatusLabel(wecomState)}
                    </span>
                  </div>
                  <div className="pl-6">
                    {activeWecomConversations.length === 0 ? (
                      <button
                        type="button"
                        onClick={openChannelOverview}
                        className="w-full rounded-md px-2.5 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                      >
                        {channelEmptyHint(wecomState)}
                      </button>
                    ) : (
                      renderChannelRows(activeWecomConversations)
                    )}
                  </div>
                </div>

                <div>
                  <div className="mb-1.5 flex items-center gap-2 px-2 text-sm font-medium text-sidebar-foreground">
                    <img
                      src="/logos/wechat.png"
                      alt=""
                      className="h-5 w-5 rounded-md"
                      draggable={false}
                    />
                    {t("channel.platforms.wechat.name")}
                    <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {channelStatusLabel(wechatState)}
                    </span>
                  </div>
                  <div className="pl-6">
                    {activeWechatConversations.length === 0 ? (
                      <button
                        type="button"
                        onClick={openChannelOverview}
                        className="w-full rounded-md px-2.5 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                      >
                        {channelEmptyHint(wechatState)}
                      </button>
                    ) : (
                      renderChannelRows(activeWechatConversations)
                    )}
                  </div>
                </div>

                <div>
                  <div className="mb-1.5 flex items-center gap-2 px-2 text-sm font-medium text-sidebar-foreground">
                    <img
                      src="/logos/telegram.png"
                      alt=""
                      className="h-5 w-5 rounded-md"
                      draggable={false}
                    />
                    {t("channel.platforms.telegram.name")}
                    <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {channelStatusLabel(telegramState)}
                    </span>
                  </div>
                  <div className="pl-6">
                    {activeTelegramConversations.length === 0 ? (
                      <button
                        type="button"
                        onClick={openChannelOverview}
                        className="w-full rounded-md px-2.5 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                      >
                        {channelEmptyHint(telegramState)}
                      </button>
                    ) : (
                      renderChannelRows(activeTelegramConversations)
                    )}
                  </div>
                </div>

                <div>
                  <div className="mb-1.5 flex items-center gap-2 px-2 text-sm font-medium text-sidebar-foreground">
                    <img
                      src="/logos/whatsapp.png"
                      alt=""
                      className="h-5 w-5 rounded-md"
                      draggable={false}
                    />
                    {t("channel.platforms.whatsapp.name")}
                    <span className="ml-auto rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                      {channelStatusLabel(whatsappState)}
                    </span>
                  </div>
                  <div className="pl-6">
                    {activeWhatsappConversations.length === 0 ? (
                      <button
                        type="button"
                        onClick={openChannelOverview}
                        className="w-full rounded-md px-2.5 py-2 text-left text-sm font-medium text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground"
                      >
                        {channelEmptyHint(whatsappState)}
                      </button>
                    ) : (
                      renderChannelRows(activeWhatsappConversations)
                    )}
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        <SidebarFooterSettings onClick={() => openSettings("account")} />
      </aside>

      {/* 重命名弹窗 */}
      <Dialog
        open={!!renamingId}
        onOpenChange={(open) => !open && setRenamingId(null)}
      >
        <DialogContent className="w-[400px]">
          <DialogHeader>
            <DialogTitle>重命名聊天</DialogTitle>
          </DialogHeader>
          <Input
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void handleRenameConfirm()}
            autoFocus
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenamingId(null)}>
              取消
            </Button>
            <Button
              onClick={() => void handleRenameConfirm()}
              disabled={!renameValue.trim()}
            >
              确认
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <DevControlPanel open={devPanelOpen} onOpenChange={setDevPanelOpen} />
    </>
  );
}
