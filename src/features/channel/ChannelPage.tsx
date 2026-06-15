import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { BellRing, Copy, Download, MoreHorizontal } from "lucide-react";
import { AppDropdown } from "@/components/common/AppDropdown";
import { requestConfirm } from "@/components/common/ConfirmDialogHost";
import { initChannelListeners, useChannelStore } from "@/stores/channelStore";
import { useChatStore } from "@/stores/chatStore";
import { ChatBottomArea } from "@/components/chat-scene/ChatBottomArea";
import { RightPanel } from "@/components/chat/RightPanel";
import { savePreviewTargetToDisk } from "@/components/chat/fileDownload";
import type { PreviewTarget } from "@/components/chat/generatedFileActions";
import { ChatArea } from "@/components/layout/ChatArea";
import { ChatTopBar } from "@/components/shell/ChatTopBar";
import { TeamChatDrawer } from "@/components/team/TeamChatDrawer";
import { Button } from "@/components/ui/button";
import { ConversationExportDialog } from "@/features/chat/ConversationExportDialog";
import { useConversationExport } from "@/hooks/useConversationExport";
import { useProductName } from "@/hooks/useProductName";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Switch } from "@/components/common/Switch";
import { getMessages, getTasks, openGeneratedFile } from "@/lib/tauri";
import type { ChannelPlatform, ChannelPlatformState } from "@/lib/tauri";
import { useNotificationStore } from "@/stores/notificationStore";
import { useTeamOverview } from "@/hooks/useTeamOverview";
import { ChannelConfig } from "./ChannelConfig";
import { ChannelConfigDetails } from "./ChannelConfigDetails";
import { FeishuChannelConfig } from "./FeishuChannelConfig";
import { TelegramChannelConfig } from "./TelegramChannelConfig";
import { WecomChannelConfig } from "./WecomChannelConfig";
import { WechatChannelConfig } from "./WechatChannelConfig";
import { WhatsappChannelConfig } from "./WhatsappChannelConfig";

interface ChannelPageProps {
  sessionId?: string;
}

type PlatformKey = ChannelPlatform;

// eslint-disable-next-line react-refresh/only-export-components
export const PLATFORM_LOGO_SRC: Record<PlatformKey, string> = {
  dingtalk: "/logos/dingtalk.png",
  feishu: "/logos/feishu.png",
  wecom: "/logos/wecom.png",
  wechat: "/logos/wechat.png",
  telegram: "/logos/telegram.png",
  whatsapp: "/logos/whatsapp.png",
};

interface PlatformCardModel {
  key: PlatformKey;
  name: string;
  description: string;
  logoSrc: string;
  state: ChannelPlatformState;
  statusLabel: string;
  statusTone: "success" | "muted" | "error" | "pending";
  networkHint: string | null;
}

function statusMeta(state: ChannelPlatformState, t: TFunction) {
  if (state.capability === "comingSoon")
    return {
      statusLabel: t("channel.status.unconfigured"),
      statusTone: "muted" as const,
    };
  if (!state.configured)
    return {
      statusLabel: t("channel.status.unconfigured"),
      statusTone: "muted" as const,
    };
  if (!state.enabled)
    return {
      statusLabel: t("channel.status.configuredOffline"),
      statusTone: "muted" as const,
    };
  switch (state.connection) {
    case "connected":
      return {
        statusLabel: t("channel.status.connected"),
        statusTone: "success" as const,
      };
    case "connecting":
      return {
        statusLabel: t("channel.status.connecting"),
        statusTone: "pending" as const,
      };
    case "reconnecting":
      return {
        statusLabel: t("channel.status.reconnecting"),
        statusTone: "pending" as const,
      };
    case "configError":
      return {
        statusLabel: t("channel.status.configError"),
        statusTone: "error" as const,
      };
    case "needsReauth":
      return {
        statusLabel: t("channel.status.sessionExpired"),
        statusTone: "error" as const,
      };
    case "disconnected":
      return {
        statusLabel: t("channel.status.disconnected"),
        statusTone: "muted" as const,
      };
    default:
      return {
        statusLabel: t("channel.status.disconnected"),
        statusTone: "muted" as const,
      };
  }
}

/**
 * 已配置 + 已启用但连不上时给一句具体的、用户可操作的提示，覆盖默认
 * "通过 XX 机器人接收并回复用户消息" 描述。
 *
 * 现在只对 Telegram / WhatsApp 触发：这两个走海外服务器，国内通常需要代理。
 * 其它渠道（钉钉/飞书/企微/微信）走国内服务器，"重连中"通常是临时问题，
 * 不给一刀切的代理提示避免误导。
 *
 * 重要：connection=disconnected 不一定是网络问题。WhatsApp 主端踢掉 web
 * session 时后端可能仍停留在 disconnected（等待 connector 翻成 needsReauth），
 * 此时再说"网络/代理不可用"是误报。所以这里的提示只描述现象 + 列出几种
 * 可能的原因，不主观断言。
 */
function networkHint(state: ChannelPlatformState, t: TFunction): string | null {
  if (!state.configured || !state.enabled) return null;
  if (state.platform !== "telegram" && state.platform !== "whatsapp")
    return null;
  const platformName = state.platform === "telegram" ? "Telegram" : "WhatsApp";
  switch (state.connection) {
    case "connecting":
      return t("channel.networkHint.connecting", { name: platformName });
    case "reconnecting":
      return t("channel.networkHint.reconnecting", { name: platformName });
    case "disconnected":
      return state.platform === "whatsapp"
        ? t("channel.networkHint.whatsappDisconnected")
        : t("channel.networkHint.telegramDisconnected");
    case "needsReauth":
      return state.platform === "whatsapp"
        ? t("channel.networkHint.whatsappNeedsReauth")
        : t("channel.networkHint.telegramNeedsReauth");
    default:
      return null;
  }
}

function StatusBadge({
  label,
  tone,
}: {
  label?: string;
  tone?: PlatformCardModel["statusTone"];
}) {
  if (!label) return null;
  const className =
    tone === "success"
      ? "bg-emerald-50 text-emerald-600"
      : tone === "error"
        ? "bg-red-50 text-red-500"
        : tone === "pending"
          ? "bg-amber-50 text-amber-600"
          : "bg-muted text-muted-foreground";
  return (
    <span className={`rounded-md px-2 py-1 text-xs font-bold ${className}`}>
      {label}
    </span>
  );
}

function PlatformIcon({ platform }: { platform: PlatformCardModel }) {
  return (
    <img
      src={platform.logoSrc}
      alt=""
      className="h-10 w-10 shrink-0 rounded-md bg-card"
      draggable={false}
    />
  );
}

function PlatformCard({
  platform,
  onRegister,
  onShowDetails,
  onRemove,
  onToggle,
  onSendGreeting,
  sendingGreeting,
}: {
  platform: PlatformCardModel;
  onRegister: () => void;
  onShowDetails: () => void;
  onRemove: () => void;
  onToggle: (enabled: boolean) => void;
  onSendGreeting?: () => void;
  sendingGreeting?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-[72px] items-center justify-between rounded-md border border-border/65 bg-card px-4 py-3 shadow-[var(--shadow-channel-item)] transition-[border-color,background-color,box-shadow] hover:border-border/80 hover:bg-card/95 hover:shadow-[var(--shadow-channel-item-hover)]">
      <div className="flex min-w-0 items-center gap-3">
        <PlatformIcon platform={platform} />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold text-foreground">
              {platform.name}
            </h3>
            <StatusBadge
              label={platform.statusLabel}
              tone={platform.statusTone}
            />
          </div>
          {platform.networkHint ? (
            <p className="mt-0.5 text-xs font-medium text-destructive">
              {platform.networkHint}
            </p>
          ) : (
            <p className="mt-0.5 text-xs font-medium text-muted-foreground">
              {platform.description}
            </p>
          )}
        </div>
      </div>

      <div className="ml-4 flex shrink-0 items-center gap-3">
        {platform.state.configured && onSendGreeting && (
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={sendingGreeting}
            aria-label={t("channel.actions.sendDingtalkGreetingAria")}
            onClick={onSendGreeting}
          >
            {sendingGreeting
              ? t("channel.actions.sendingGreeting")
              : t("channel.actions.sendGreeting")}
          </Button>
        )}
        {platform.state.configured && (
          <AppDropdown
            ariaLabel={t("channel.actions.morePlatformConfig", {
              name: platform.name,
            })}
            trigger={
              <Button unstyled
                type="button"
                className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
              >
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            }
            items={[
              {
                id: "configure",
                label: t("channel.actions.configure"),
                onSelect: onShowDetails,
              },
              {
                id: "remove",
                label: t("channel.actions.remove"),
                className: "text-destructive",
                onSelect: onRemove,
              },
            ]}
          />
        )}
        {platform.state.configured ? (
          <Switch
            checked={platform.state.enabled}
            aria-label={
              platform.state.enabled
                ? t("channel.actions.enabledAria", { name: platform.name })
                : t("channel.actions.disabledAria", { name: platform.name })
            }
            onCheckedChange={onToggle}
          />
        ) : platform.state.capability === "available" ? (
          <Button
            type="button"
            size="sm"
            onClick={onRegister}
            aria-label={t("channel.actions.configureWith", {
              name: platform.name,
            })}
          >
            {t("channel.actions.configure")}
          </Button>
        ) : (
          <Button type="button" size="sm" disabled>
            {t("channel.actions.configure")}
          </Button>
        )}
      </div>
    </div>
  );
}

function ChannelHero() {
  const { t } = useTranslation();
  const productName = useProductName();
  return (
    <div className="flex flex-col">
      <h1 className="text-[22px] font-bold leading-7 text-foreground">
        {t("channel.heroTitle")}
      </h1>
      <p className="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">
        {t("channel.heroDesc", { productName })} {t("channel.heroPrivacy")}
      </p>
    </div>
  );
}

function ChannelOverview({
  platforms,
  onRegisterDingtalk,
  onShowDingtalkDetails,
  onRemoveDingtalk,
  onToggleDingtalk,
  onSendDingtalkGreeting,
  sendingDingtalkGreeting,
  onRegisterFeishu,
  onShowFeishuDetails,
  onRemoveFeishu,
  onToggleFeishu,
  onRegisterWecom,
  onShowWecomDetails,
  onRemoveWecom,
  onToggleWecom,
  onRegisterWechat,
  onShowWechatDetails,
  onRemoveWechat,
  onToggleWechat,
  onRegisterTelegram,
  onShowTelegramDetails,
  onRemoveTelegram,
  onToggleTelegram,
  onRegisterWhatsapp,
  onShowWhatsappDetails,
  onRemoveWhatsapp,
  onToggleWhatsapp,
}: {
  platforms: PlatformCardModel[];
  onRegisterDingtalk: () => void;
  onShowDingtalkDetails: () => void;
  onRemoveDingtalk: () => void;
  onToggleDingtalk: (enabled: boolean) => void;
  onSendDingtalkGreeting: () => void;
  sendingDingtalkGreeting: boolean;
  onRegisterFeishu: () => void;
  onShowFeishuDetails: () => void;
  onRemoveFeishu: () => void;
  onToggleFeishu: (enabled: boolean) => void;
  onRegisterWecom: () => void;
  onShowWecomDetails: () => void;
  onRemoveWecom: () => void;
  onToggleWecom: (enabled: boolean) => void;
  onRegisterWechat: () => void;
  onShowWechatDetails: () => void;
  onRemoveWechat: () => void;
  onToggleWechat: (enabled: boolean) => void;
  onRegisterTelegram: () => void;
  /** 已配对后 kebab "配置" 入口 —— 复用注册对话框,组件内部按 alreadyConfigured 渲染管理界面。 */
  onShowTelegramDetails: () => void;
  onRemoveTelegram: () => void;
  onToggleTelegram: (enabled: boolean) => void;
  onRegisterWhatsapp: () => void;
  /** 同上,WhatsApp 二次编辑入口,对话框按 connected 显示"允许列表"管理。 */
  onShowWhatsappDetails: () => void;
  onRemoveWhatsapp: () => void;
  onToggleWhatsapp: (enabled: boolean) => void;
}) {
  const { t } = useTranslation();
  const noop = () => {};
  const noopToggle = () => {};
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div
        data-tauri-drag-region
        className="flex h-12 shrink-0 items-center border-b border-border px-8"
      >
        <span className="text-[15px] font-semibold leading-[22px] text-foreground">
          {t("nav.channel")}
        </span>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="mx-auto flex w-full max-w-[960px] flex-col gap-6 px-8 py-7">
          <ChannelHero />

          <div className="flex flex-col gap-3">
            {platforms.map((platform) => (
              <PlatformCard
                key={platform.key}
                platform={platform}
                onRegister={
                  platform.key === "dingtalk"
                    ? onRegisterDingtalk
                    : platform.key === "feishu"
                      ? onRegisterFeishu
                      : platform.key === "wecom"
                        ? onRegisterWecom
                        : platform.key === "wechat"
                          ? onRegisterWechat
                          : platform.key === "telegram"
                            ? onRegisterTelegram
                            : platform.key === "whatsapp"
                              ? onRegisterWhatsapp
                              : noop
                }
                onShowDetails={
                  platform.key === "dingtalk"
                    ? onShowDingtalkDetails
                    : platform.key === "feishu"
                      ? onShowFeishuDetails
                      : platform.key === "wecom"
                        ? onShowWecomDetails
                        : platform.key === "wechat"
                          ? onShowWechatDetails
                          : platform.key === "telegram"
                            ? onShowTelegramDetails
                            : platform.key === "whatsapp"
                              ? onShowWhatsappDetails
                              : noop
                }
                onRemove={
                  platform.key === "dingtalk"
                    ? onRemoveDingtalk
                    : platform.key === "feishu"
                      ? onRemoveFeishu
                      : platform.key === "wecom"
                        ? onRemoveWecom
                        : platform.key === "wechat"
                          ? onRemoveWechat
                          : platform.key === "telegram"
                            ? onRemoveTelegram
                            : platform.key === "whatsapp"
                              ? onRemoveWhatsapp
                              : noop
                }
                onToggle={
                  platform.key === "dingtalk"
                    ? onToggleDingtalk
                    : platform.key === "feishu"
                      ? onToggleFeishu
                      : platform.key === "wecom"
                        ? onToggleWecom
                        : platform.key === "wechat"
                          ? onToggleWechat
                          : platform.key === "telegram"
                            ? onToggleTelegram
                            : platform.key === "whatsapp"
                              ? onToggleWhatsapp
                              : noopToggle
                }
                onSendGreeting={
                  platform.key === "dingtalk" ? onSendDingtalkGreeting : undefined
                }
                sendingGreeting={
                  platform.key === "dingtalk" ? sendingDingtalkGreeting : false
                }
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function ChannelChatView({ sessionId }: { sessionId: string }) {
  const { t } = useTranslation();
  const conversations = useChannelStore((s) => s.conversations);
  const sendDingtalkGreeting = useChannelStore((s) => s.sendDingtalkGreeting);
  const pushNotification = useNotificationStore((s) => s.push);
  const [sendingDingtalkGreeting, setSendingDingtalkGreeting] =
    useState(false);
  const activeConv = conversations.find((c) => c.sessionId === sessionId);
  // 顶栏副标题：直接展示后端填的 displayName（= sender push_name / nick / userid）。
  // 飞书 / 个人微信目前后端拿不到真实用户名（飞书是 "飞书用户 ou_xxx" 占位，
  // 微信是裸 wxid_xxx 或 openid@im.wechat），展示无价值，整段 workspace 隐藏；
  // 等后端补上真实用户名时把 platform 从 HIDE_WORKSPACE_PLATFORMS 删掉即可。
  const HIDE_WORKSPACE_PLATFORMS = new Set(["feishu", "wechat"]);
  const title = activeConv
    ? activeConv.displayName?.trim() ||
      (activeConv.conversationType === "group"
        ? t("channel.chat.groupChat")
        : t("channel.chat.privateChat"))
    : "";
  const workspaceLabel =
    activeConv && HIDE_WORKSPACE_PLATFORMS.has(activeConv.platform)
      ? undefined
      : title || sessionId;
  const platformTitle = activeConv
    ? (t(`channel.platforms.${activeConv.platform}.name`) ??
      activeConv.platform)
    : "";
  const isInactiveSession = !!activeConv && !activeConv.isActiveRobot;
  const canWakeDingtalk =
    activeConv?.platform === "dingtalk" && activeConv.isActiveRobot;
  const { overview: teamOverview } = useTeamOverview(sessionId);
  const conversationExport = useConversationExport(sessionId);

  const handleCopyConversationId = () => {
    void navigator.clipboard.writeText(sessionId);
  };

  const moreMenuItems = [
    {
      id: "export",
      label: t("chatHeader.exportConversation", "导出对话"),
      icon: <Download />,
      onSelect: conversationExport.openExportDialog,
    },
    {
      id: "copy-id",
      label: t("sidebar.copyConversationId"),
      icon: <Copy />,
      onSelect: handleCopyConversationId,
    },
  ];

  const handleSendDingtalkGreeting = async () => {
    if (sendingDingtalkGreeting) return;
    setSendingDingtalkGreeting(true);
    try {
      await sendDingtalkGreeting();
      pushNotification({
        level: "success",
        title: t("channel.dingtalk.greeting.sentTitle"),
        message: t("channel.dingtalk.greeting.sentMessage"),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: "toast",
      });
    } catch (error) {
      pushNotification({
        level: "error",
        title: t("channel.dingtalk.greeting.failedTitle"),
        message:
          error instanceof Error
            ? error.message
            : t("channel.dingtalk.greeting.failedMessage"),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: "toast",
      });
    } finally {
      setSendingDingtalkGreeting(false);
    }
  };

  const handleOpenPreviewTarget = async (target: PreviewTarget) => {
    try {
      await openGeneratedFile(target.fileId, target.conversationId);
    } catch (err) {
      pushNotification({
        level: "error",
        title: t("channel.errors.openFileTitle"),
        message:
          err instanceof Error
            ? err.message
            : t("channel.errors.openFileMessage"),
        actions: [],
        dismissible: true,
        context: "toast",
      });
    }
  };

  const handleDownloadPreviewTarget = async (target: PreviewTarget) => {
    try {
      const savedPath = await savePreviewTargetToDisk(target);
      if (!savedPath) return;
      pushNotification({
        level: "success",
        title: t("messageList.fileDownloaded", "已下载文件"),
        message: savedPath,
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: "toast",
      });
    } catch (err) {
      pushNotification({
        level: "error",
        title: t("messageList.cannotDownload", "无法下载文件"),
        message:
          err instanceof Error
            ? err.message
            : t("channel.errors.openFileMessage"),
        actions: [],
        dismissible: true,
        context: "toast",
      });
    }
  };

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background">
      <ChatTopBar
        title={platformTitle}
        workspace={workspaceLabel}
        trailing={
          canWakeDingtalk ? (
            <Button
              type="button"
              size="sm"
              variant="secondary"
              icon={<BellRing />}
              loading={sendingDingtalkGreeting}
              aria-label={t("channel.actions.sendDingtalkGreetingAria")}
              onClick={() => void handleSendDingtalkGreeting()}
            >
              {sendingDingtalkGreeting
                ? t("channel.actions.sendingGreeting")
                : t("channel.actions.sendGreeting")}
            </Button>
          ) : undefined
        }
        moreMenuItems={moreMenuItems}
      />
      <div className="relative flex min-h-0 flex-1 overflow-hidden">
        <div
          data-testid="channel-chat-layout-column"
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
        >
          <ChatArea />
          {isInactiveSession && (
            <div className="px-6 pb-2">
              <div className="rounded-md bg-muted px-3 py-2 text-sm text-muted-foreground">
                {t("channel.chat.inactiveBanner")}
              </div>
            </div>
          )}
          <ChatBottomArea
            disabled={isInactiveSession}
            sessionIdOverride={sessionId}
          />
        </div>
        <TeamChatDrawer conversationId={sessionId} overview={teamOverview} />
        <RightPanel
          conversationId={sessionId}
          onOpenExternal={(target) => void handleOpenPreviewTarget(target)}
          onDownload={(target) => void handleDownloadPreviewTarget(target)}
        />
      </div>
      <ConversationExportDialog {...conversationExport.dialogProps} />
    </div>
  );
}

export function ChannelPage({ sessionId }: ChannelPageProps) {
  const { t } = useTranslation();
  const platformsByKey = useChannelStore((s) => s.platforms);
  const loadConversations = useChannelStore((s) => s.loadConversations);
  const setEnabled = useChannelStore((s) => s.setEnabled);
  const removePlatform = useChannelStore((s) => s.removePlatform);
  const sendDingtalkGreeting = useChannelStore((s) => s.sendDingtalkGreeting);
  const pushNotification = useNotificationStore((s) => s.push);
  const [registrationOpen, setRegistrationOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [sendingDingtalkGreeting, setSendingDingtalkGreeting] = useState(false);
  const [feishuRegistrationOpen, setFeishuRegistrationOpen] = useState(false);
  const [feishuDetailsOpen, setFeishuDetailsOpen] = useState(false);
  const [wecomRegistrationOpen, setWecomRegistrationOpen] = useState(false);
  const [wecomDetailsOpen, setWecomDetailsOpen] = useState(false);
  const [wechatRegistrationOpen, setWechatRegistrationOpen] = useState(false);
  const [wechatDetailsOpen, setWechatDetailsOpen] = useState(false);
  const [telegramRegistrationOpen, setTelegramRegistrationOpen] =
    useState(false);
  const [whatsappRegistrationOpen, setWhatsappRegistrationOpen] =
    useState(false);

  useEffect(() => {
    void initChannelListeners();
    void loadConversations();
  }, [loadConversations]);

  useEffect(() => {
    const store = useChatStore.getState();
    const activeId = sessionId ?? null;

    if (!activeId) {
      if (store.activeConversationId !== null) {
        store.setMessages([]);
      }
      return;
    }

    // Selecting a session = the user has seen the new messages → reset the
    // unread badge. `incrementUnread` fires on every `channel:message` event
    // when this session isn't the active one (see channelStore.ts:171),
    // so without this counter clear the badge would grow forever.
    useChannelStore.getState().clearUnread(activeId);

    let cancelled = false;
    store.setMessages([]);

    void Promise.all([
      getMessages(activeId),
      getTasks(activeId).catch(() => []),
    ])
      .then(([messages, tasks]) => {
        if (cancelled) return;
        const latest = useChatStore.getState();
        latest.setMessages(messages);
        for (const task of tasks) {
          latest.upsertConversationTaskState(activeId, task);
        }
      })
      .catch((err) => {
        if (!cancelled)
          console.error("[ChannelPage] load channel session failed", err);
      });

    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const dingtalkState =
    platformsByKey.dingtalk ??
    ({
      platform: "dingtalk",
      capability: "available",
      configured: false,
      enabled: false,
      connection: "unconfigured",
      config: null,
      lastConnectedAt: null,
      lastError: null,
    } satisfies ChannelPlatformState);

  const feishuState =
    platformsByKey.feishu ??
    ({
      platform: "feishu",
      capability: "available",
      configured: false,
      enabled: false,
      connection: "unconfigured",
      config: null,
      lastConnectedAt: null,
      lastError: null,
    } satisfies ChannelPlatformState);

  const wecomState =
    platformsByKey.wecom ??
    ({
      platform: "wecom",
      capability: "available",
      configured: false,
      enabled: false,
      connection: "unconfigured",
      config: null,
      lastConnectedAt: null,
      lastError: null,
    } satisfies ChannelPlatformState);

  const wechatState =
    platformsByKey.wechat ??
    ({
      platform: "wechat",
      // MVP: report as available so the user can click the button to drive a
      // real scan flow. Backend persistence still lands in Phase 5 PR3, so the
      // card never reaches "configured" state in this MVP cut.
      capability: "available",
      configured: false,
      enabled: false,
      connection: "unconfigured",
      config: null,
      lastConnectedAt: null,
      lastError: null,
    } satisfies ChannelPlatformState);

  const telegramState =
    platformsByKey.telegram ??
    ({
      platform: "telegram",
      capability: "available",
      configured: false,
      enabled: false,
      connection: "unconfigured",
      config: null,
      lastConnectedAt: null,
      lastError: null,
    } satisfies ChannelPlatformState);

  const whatsappState =
    platformsByKey.whatsapp ??
    ({
      platform: "whatsapp",
      capability: "comingSoon",
      configured: false,
      enabled: false,
      connection: "unconfigured",
      config: null,
      lastConnectedAt: null,
      lastError: null,
    } satisfies ChannelPlatformState);

  const handleRemoveDingtalk = async () => {
    const confirmed = await requestConfirm({
      title: t("channel.remove.dingtalk.title"),
      description: t("channel.remove.dingtalk.description"),
      confirmLabel: t("channel.actions.confirmRemove"),
      cancelLabel: t("channel.actions.cancel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    await removePlatform("dingtalk");
  };

  const handleToggleDingtalk = async (enabled: boolean) => {
    await setEnabled("dingtalk", enabled);
  };

  const handleSendDingtalkGreeting = async () => {
    if (sendingDingtalkGreeting) return;
    setSendingDingtalkGreeting(true);
    try {
      await sendDingtalkGreeting();
      pushNotification({
        level: "success",
        title: t("channel.dingtalk.greeting.sentTitle"),
        message: t("channel.dingtalk.greeting.sentMessage"),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: "toast",
      });
    } catch (error) {
      pushNotification({
        level: "error",
        title: t("channel.dingtalk.greeting.failedTitle"),
        message:
          error instanceof Error
            ? error.message
            : t("channel.dingtalk.greeting.failedMessage"),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: "toast",
      });
    } finally {
      setSendingDingtalkGreeting(false);
    }
  };

  const handleRemoveFeishu = async () => {
    const confirmed = await requestConfirm({
      title: t("channel.remove.feishu.title"),
      description: t("channel.remove.feishu.description"),
      confirmLabel: t("channel.actions.confirmRemove"),
      cancelLabel: t("channel.actions.cancel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    await removePlatform("feishu");
  };

  const handleToggleFeishu = async (enabled: boolean) => {
    await setEnabled("feishu", enabled);
  };

  const handleRemoveWecom = async () => {
    const confirmed = await requestConfirm({
      title: t("channel.remove.wecom.title"),
      description: t("channel.remove.wecom.description"),
      confirmLabel: t("channel.actions.confirmRemove"),
      cancelLabel: t("channel.actions.cancel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    await removePlatform("wecom");
  };

  const handleToggleWecom = async (enabled: boolean) => {
    await setEnabled("wecom", enabled);
  };

  const handleRemoveWechat = async () => {
    const confirmed = await requestConfirm({
      title: t("channel.remove.wechat.title"),
      description: t("channel.remove.wechat.description"),
      confirmLabel: t("channel.actions.confirmRemove"),
      cancelLabel: t("channel.actions.cancel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    await removePlatform("wechat");
  };

  const handleToggleWechat = async (enabled: boolean) => {
    await setEnabled("wechat", enabled);
  };

  const handleRemoveTelegram = async () => {
    const confirmed = await requestConfirm({
      title: t("channel.remove.telegram.title"),
      description: t("channel.remove.telegram.description"),
      confirmLabel: t("channel.actions.confirmRemove"),
      cancelLabel: t("channel.actions.cancel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    await removePlatform("telegram");
  };

  const handleToggleTelegram = async (enabled: boolean) => {
    await setEnabled("telegram", enabled);
  };

  const handleRemoveWhatsapp = async () => {
    const confirmed = await requestConfirm({
      title: t("channel.remove.whatsapp.title"),
      description: t("channel.remove.whatsapp.description"),
      confirmLabel: t("channel.actions.confirmRemove"),
      cancelLabel: t("channel.actions.cancel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    await removePlatform("whatsapp");
  };

  const handleToggleWhatsapp = async (enabled: boolean) => {
    await setEnabled("whatsapp", enabled);
  };

  const platforms = useMemo<PlatformCardModel[]>(() => {
    const states: Record<PlatformKey, ChannelPlatformState> = {
      dingtalk: dingtalkState,
      feishu: feishuState,
      wecom: wecomState,
      wechat: wechatState,
      telegram: telegramState,
      whatsapp: whatsappState,
    };

    return [
      {
        key: "dingtalk",
        name: t("channel.platforms.dingtalk.name"),
        description: t("channel.platforms.dingtalk.description"),
        logoSrc: PLATFORM_LOGO_SRC.dingtalk,
        state: states.dingtalk,
        networkHint: networkHint(states.dingtalk, t),
        ...statusMeta(states.dingtalk, t),
      },
      {
        key: "feishu",
        name: t("channel.platforms.feishu.name"),
        description: t("channel.platforms.feishu.description"),
        logoSrc: PLATFORM_LOGO_SRC.feishu,
        state: states.feishu,
        networkHint: networkHint(states.feishu, t),
        ...statusMeta(states.feishu, t),
      },
      {
        key: "wecom",
        name: t("channel.platforms.wecom.name"),
        description: t("channel.platforms.wecom.description"),
        logoSrc: PLATFORM_LOGO_SRC.wecom,
        state: states.wecom,
        networkHint: networkHint(states.wecom, t),
        ...statusMeta(states.wecom, t),
      },
      {
        key: "wechat",
        name: t("channel.platforms.wechat.name"),
        description: t("channel.platforms.wechat.description"),
        logoSrc: PLATFORM_LOGO_SRC.wechat,
        state: states.wechat,
        networkHint: networkHint(states.wechat, t),
        ...statusMeta(states.wechat, t),
      },
      {
        key: "telegram",
        name: t("channel.platforms.telegram.name"),
        description: t("channel.platforms.telegram.description"),
        logoSrc: PLATFORM_LOGO_SRC.telegram,
        state: states.telegram,
        networkHint: networkHint(states.telegram, t),
        ...statusMeta(states.telegram, t),
      },
      {
        key: "whatsapp",
        name: t("channel.platforms.whatsapp.name"),
        description: t("channel.platforms.whatsapp.description"),
        logoSrc: PLATFORM_LOGO_SRC.whatsapp,
        state: states.whatsapp,
        networkHint: networkHint(states.whatsapp, t),
        ...statusMeta(states.whatsapp, t),
      },
    ];
  }, [
    dingtalkState,
    feishuState,
    wecomState,
    wechatState,
    telegramState,
    whatsappState,
    t,
  ]);

  return (
    <div
      className={
        sessionId
          ? "h-full overflow-hidden bg-background"
          : "h-full overflow-y-auto bg-background"
      }
    >
      {sessionId ? (
        <ChannelChatView sessionId={sessionId} />
      ) : (
        <ChannelOverview
          platforms={platforms}
          onRegisterDingtalk={() => setRegistrationOpen(true)}
          onShowDingtalkDetails={() => setDetailsOpen(true)}
          onRemoveDingtalk={() => void handleRemoveDingtalk()}
          onToggleDingtalk={(enabled) => void handleToggleDingtalk(enabled)}
          onSendDingtalkGreeting={() => void handleSendDingtalkGreeting()}
          sendingDingtalkGreeting={sendingDingtalkGreeting}
          onRegisterFeishu={() => setFeishuRegistrationOpen(true)}
          onShowFeishuDetails={() => setFeishuDetailsOpen(true)}
          onRemoveFeishu={() => void handleRemoveFeishu()}
          onToggleFeishu={(enabled) => void handleToggleFeishu(enabled)}
          onRegisterWecom={() => setWecomRegistrationOpen(true)}
          onShowWecomDetails={() => setWecomDetailsOpen(true)}
          onRemoveWecom={() => void handleRemoveWecom()}
          onToggleWecom={(enabled) => void handleToggleWecom(enabled)}
          onRegisterWechat={() => setWechatRegistrationOpen(true)}
          onShowWechatDetails={() => setWechatDetailsOpen(true)}
          onRemoveWechat={() => void handleRemoveWechat()}
          onToggleWechat={(enabled) => void handleToggleWechat(enabled)}
          onRegisterTelegram={() => setTelegramRegistrationOpen(true)}
          // 已配置后的 kebab "配置" 入口复用同一个对话框 ——
          // TelegramChannelConfig 检测到 alreadyConfigured 会跳过 token 步骤,
          // 直接进入"扫码配对 / 已连接用户管理 / 移除整个频道"管理界面。
          onShowTelegramDetails={() => setTelegramRegistrationOpen(true)}
          onRemoveTelegram={() => void handleRemoveTelegram()}
          onToggleTelegram={(enabled) => void handleToggleTelegram(enabled)}
          onRegisterWhatsapp={() => setWhatsappRegistrationOpen(true)}
          // 同 telegram —— WhatsappChannelConfig 收到 connected=true 时显示
          // "允许的发送人(E.164)"管理区域,允许编辑 allow_from 列表。
          onShowWhatsappDetails={() => setWhatsappRegistrationOpen(true)}
          onRemoveWhatsapp={() => void handleRemoveWhatsapp()}
          onToggleWhatsapp={(enabled) => void handleToggleWhatsapp(enabled)}
        />
      )}

      <Dialog open={registrationOpen} onOpenChange={setRegistrationOpen}>
        <DialogContent className="max-w-xl overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t("channel.dialog.dingtalk.title")}</DialogTitle>
            <DialogDescription>
              {t("channel.dialog.dingtalk.description")}
            </DialogDescription>
          </DialogHeader>
          <ChannelConfig
            onSaved={() => {
              void loadConversations();
            }}
            onClose={() => setRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog
        open={feishuRegistrationOpen}
        onOpenChange={setFeishuRegistrationOpen}
      >
        <DialogContent className="max-w-xl overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t("channel.dialog.feishu.title")}</DialogTitle>
            <DialogDescription>
              {t("channel.dialog.feishu.description")}
            </DialogDescription>
          </DialogHeader>
          <FeishuChannelConfig
            onSaved={() => {
              void loadConversations();
            }}
            onClose={() => setFeishuRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog
        open={wecomRegistrationOpen}
        onOpenChange={setWecomRegistrationOpen}
      >
        <DialogContent className="max-w-xl overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t("channel.dialog.wecom.title")}</DialogTitle>
            <DialogDescription>
              {t("channel.dialog.wecom.description")}
            </DialogDescription>
          </DialogHeader>
          <WecomChannelConfig
            onSaved={() => {
              void loadConversations();
            }}
            onClose={() => setWecomRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog
        open={wechatRegistrationOpen}
        onOpenChange={setWechatRegistrationOpen}
      >
        <DialogContent className="max-w-xl overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t("channel.dialog.wechat.title")}</DialogTitle>
            <DialogDescription>
              {t("channel.dialog.wechat.description")}
            </DialogDescription>
          </DialogHeader>
          <WechatChannelConfig
            onSaved={() => {
              void loadConversations();
            }}
            onClose={() => setWechatRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog
        open={telegramRegistrationOpen}
        onOpenChange={setTelegramRegistrationOpen}
      >
        <DialogContent className="max-w-xl overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t("channel.dialog.telegram.title")}</DialogTitle>
            <DialogDescription>
              {t("channel.dialog.telegram.description")}
            </DialogDescription>
          </DialogHeader>
          <TelegramChannelConfig
            onSaved={() => {
              void loadConversations();
            }}
            onClose={() => setTelegramRegistrationOpen(false)}
          />
        </DialogContent>
      </Dialog>

      <Dialog
        open={whatsappRegistrationOpen}
        onOpenChange={setWhatsappRegistrationOpen}
      >
        <DialogContent className="max-w-xl overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
          <DialogHeader className="sr-only">
            <DialogTitle>{t("channel.dialog.whatsapp.title")}</DialogTitle>
            <DialogDescription>
              {t("channel.dialog.whatsapp.description")}
            </DialogDescription>
          </DialogHeader>
          <WhatsappChannelConfig
            onSaved={() => {
              void loadConversations();
            }}
            onClose={() => setWhatsappRegistrationOpen(false)}
            connected={
              whatsappState.connection === "connected" ||
              whatsappState.connection === "reconnecting"
            }
          />
        </DialogContent>
      </Dialog>

      {dingtalkState.config && (
        <ChannelConfigDetails
          config={dingtalkState.config}
          open={detailsOpen}
          onOpenChange={setDetailsOpen}
        />
      )}
      {feishuState.config && (
        <ChannelConfigDetails
          config={feishuState.config}
          open={feishuDetailsOpen}
          onOpenChange={setFeishuDetailsOpen}
        />
      )}
      {wecomState.config && (
        <ChannelConfigDetails
          config={wecomState.config}
          open={wecomDetailsOpen}
          onOpenChange={setWecomDetailsOpen}
        />
      )}
      {wechatState.config && (
        <ChannelConfigDetails
          config={wechatState.config}
          open={wechatDetailsOpen}
          onOpenChange={setWechatDetailsOpen}
        />
      )}
    </div>
  );
}
