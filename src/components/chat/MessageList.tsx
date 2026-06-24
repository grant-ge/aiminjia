/**
 * @designSource design.pen#F8ixG flow
 * @sizing padding [24,40] gap 18
 */
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, MessageCircleQuestion } from "lucide-react";

import { AiBubble } from "@/components/chat/AiBubble";
import { CompactBoundaryBar } from "@/components/chat/CompactBoundaryBar";
import { StreamingBubble } from "@/components/chat/StreamingBubble";
import { savePreviewTargetToDisk } from "@/components/chat/fileDownload";
import { ChatRow } from "@/components/chat-scene/ChatRow";
import { GeneratedFileCard } from "@/components/chat-scene/GeneratedFileCard";
import { PeerMessageBanner } from "@/components/chat-scene/PeerMessageBanner";
import { parseDispatchHeader } from "@/components/chat-scene/parseDispatchHeader";
import { SuggestChipGroup } from "@/components/chat-scene/SuggestChipGroup";
import { ToolStepGroupBlock } from "@/components/chat-scene/ToolStepGroupBlock";
import { ToolTraceIO } from "@/components/chat-scene/ToolTraceIO";
import { UserMessageBubble } from "@/components/chat-scene/UserMessageBubble";
import { TeamProgressBlock } from "@/components/team/TeamProgressBlock";
import { TeamVisualProvider } from "@/components/team/TeamVisualContext";
import { toPreviewTarget } from "@/components/chat/generatedFileActions";
import type { ExpertTeamId } from "@/features/expert-teams/teams";
import { getExpertTeam } from "@/features/expert-teams/teams";
import { useAuthStore } from "@/stores/authStore";
import { useBrandingStore } from "@/stores/brandingStore";
import { useChannelStore } from "@/stores/channelStore";
import { useChatStore } from "@/stores/chatStore";
import { useGeneratedFilePreviewStore } from "@/stores/generatedFilePreviewStore";
import { useNotificationStore } from "@/stores/notificationStore";
import { useChat } from "@/hooks/useChat";
import { useTeamOverview } from "@/hooks/useTeamOverview";
import {
  useTurnRenderModel,
  type RenderAiSegment,
  type RenderGeneratedFile,
  type RenderToolGroup,
  type RenderToolReceipt,
  type RenderToolStep,
  type RenderTurnBlock,
} from "@/hooks/useTurnRenderModel";
import {
  openGeneratedFile,
  openLocalFile,
  revealFileInFolder,
} from "@/lib/tauri";
import { useConversationTeamState, useTeamStore } from "@/stores/teamStore";
import { Button } from '@/components/ui/button'

type FileActionKind = "preview" | "open" | "download" | "reveal";

type GeneratedFileCardProps = Parameters<typeof GeneratedFileCard>[0];

function AvailableGeneratedFileCard({
  ...cardProps
}: GeneratedFileCardProps & { file: RenderGeneratedFile }) {
  return <GeneratedFileCard {...cardProps} />;
}

function CompletedProcessCollapse({
  children,
  toolGroup,
}: {
  children: ReactNode;
  toolGroup?: RenderToolGroup;
}) {
  const [open, setOpen] = useState(false);
  const stepLabel = toolGroup?.steps.length
    ? `${toolGroup.steps.length} 步`
    : null;
  const summaryLabel = ["已完成", stepLabel].filter(Boolean).join(" · ");

  return (
    <div>
      <Button
        unstyled
        type="button"
        aria-label={summaryLabel}
        onClick={() => setOpen((value) => !value)}
        className="inline-flex w-fit max-w-full min-w-0 items-center gap-1.5 py-1.5 text-left text-xs text-muted-foreground hover:text-foreground"
      >
        <span className="min-w-0 break-words">{summaryLabel}</span>
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
        )}
      </Button>
      {open ? (
        <div data-testid="completed-process-body" className="mt-1 flex flex-col gap-1">
          {children}
        </div>
      ) : null}
    </div>
  );
}

// Display name for IM platforms when the inbound conversation's sender is
// rendered as the user-side identity. Keep in sync with AppSidebar's
// CHANNEL_PLATFORM_NAME — WhatsApp intentionally not in this map because it
// uses the contact's real push_name rather than the platform brand.
const CHANNEL_PLATFORM_DISPLAY: Record<string, string> = {
  dingtalk: "钉钉",
  feishu: "飞书",
  wecom: "企业微信",
  wechat: "个人微信",
  telegram: "Telegram",
};

interface MessageListProps {
  expertTeamId?: ExpertTeamId;
}

function ToolReceiptBlock({
  receipt,
  step,
}: {
  receipt: RenderToolReceipt;
  step?: RenderToolStep;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  if (receipt.kind === "interaction") {
    const questionCount = receipt.questionCount ?? receipt.items?.length ?? 0;
    const title =
      receipt.status === "cancelled"
        ? t("messageList.toolReceipt.interactionCancelled")
        : t("messageList.toolReceipt.interactionAsked", {
            count: questionCount,
          });
    const received = receipt.summary
      ? t("messageList.toolReceipt.interactionReceived", {
          summary: receipt.summary,
        })
      : undefined;

    return (
      <div className="flex max-w-full flex-col gap-2 py-1 text-sm">
        {step ? (
          <Button unstyled
            type="button"
            aria-label={title}
            onClick={() => setOpen((value) => !value)}
            className="inline-flex w-fit max-w-full min-w-0 items-center gap-1.5 text-left text-muted-foreground hover:text-foreground"
          >
            <MessageCircleQuestion
              className="h-3.5 w-3.5 shrink-0"
              aria-hidden="true"
            />
            <span className="min-w-0 break-words">{title}</span>
            {open ? (
              <ChevronDown
                className="h-3.5 w-3.5 shrink-0"
                aria-hidden="true"
              />
            ) : (
              <ChevronRight
                className="h-3.5 w-3.5 shrink-0"
                aria-hidden="true"
              />
            )}
          </Button>
        ) : (
          <div className="inline-flex min-w-0 items-center gap-1.5 text-muted-foreground">
            <MessageCircleQuestion
              className="h-3.5 w-3.5 shrink-0"
              aria-hidden="true"
            />
            <span className="min-w-0 break-words">{title}</span>
          </div>
        )}
        {received ? (
          <div className="min-w-0 break-words text-foreground">{received}</div>
        ) : receipt.items?.length ? (
          <dl className="grid gap-1 text-foreground">
            {receipt.items.map((item, index) => (
              <div
                key={`${item.label ?? index}-${item.value}`}
                className="flex min-w-0 gap-1.5"
              >
                {item.label ? (
                  <dt className="shrink-0 text-muted-foreground">
                    {item.label}:
                  </dt>
                ) : null}
                <dd className="min-w-0 whitespace-pre-wrap break-words">
                  {item.value}
                </dd>
              </div>
            ))}
          </dl>
        ) : receipt.message ? (
          <div className="whitespace-pre-wrap break-words text-foreground">
            {receipt.message}
          </div>
        ) : null}
        {open && step ? (
          <div className="mt-1">
            <ToolTraceIO
              toolName={step.name}
              inputJson={step.inputJson}
              output={step.output}
            />
          </div>
        ) : null}
      </div>
    );
  }

  const title = (() => {
    switch (receipt.status) {
      case "approved":
        return t("messageList.toolReceipt.permissionApproved");
      case "cancelled":
        return t("messageList.toolReceipt.permissionCancelled");
      default:
        return t("messageList.toolReceipt.permissionDenied");
    }
  })();

  return (
    <div className="flex max-w-full flex-col gap-1 overflow-hidden rounded-md bg-muted/45 px-3 py-2 text-sm text-muted-foreground">
      <div className="text-foreground">{title}</div>
      {receipt.summary ? (
        <div className="line-clamp-2 min-w-0 break-words text-foreground">
          {receipt.summary}
        </div>
      ) : receipt.items?.length ? (
        <dl className="grid gap-1">
          {receipt.items.map((item, index) => (
            <div
              key={`${item.label ?? index}-${item.value}`}
              className="flex min-w-0 gap-1.5"
            >
              {item.label ? (
                <dt className="shrink-0 text-muted-foreground">
                  {item.label}:
                </dt>
              ) : null}
              <dd className="min-w-0 whitespace-pre-wrap break-words text-foreground">
                {item.value}
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
      {receipt.message ? (
        <div className="whitespace-pre-wrap break-words text-foreground">
          {receipt.message}
        </div>
      ) : null}
    </div>
  );
}

export function MessageList({ expertTeamId }: MessageListProps = {}) {
  const { t, i18n } = useTranslation();
  const expertTeam = expertTeamId
    ? getExpertTeam(expertTeamId, i18n.language)
    : undefined;

  const FILE_ACTION_ERROR_TITLES: Record<FileActionKind, string> = {
    preview: t("messageList.cannotPreview"),
    open: t("messageList.cannotOpen"),
    download: t("messageList.cannotDownload", "无法下载文件"),
    reveal: t("messageList.cannotReveal"),
  };

  const turns = useTurnRenderModel();
  useChat();
  const activeConversationId = useChatStore((s) => s.activeConversationId);
  const isStreaming = useChatStore((s) => s.isStreaming);
  // Show the streaming bubble whenever this conversation is "busy" — that
  // window is wider than `isStreaming` alone (covers sentinel resume + the
  // moment between `addBusyConversation` and the first stream event), which
  // closes the visual gap users see today (spec §6.4).
  const showStreamingBubble = useChatStore((s) => {
    const id = s.activeConversationId;
    if (!id) return false;
    return (
      s.busyConversations.has(id) || (s.streamStates[id]?.isStreaming ?? false)
    );
  });
  const streamingContent = useChatStore((s) => {
    const activeId = s.activeConversationId;
    return activeId ? (s.streamStates[activeId]?.streamingContent ?? "") : "";
  });
  const lastCompactSummary = useChatStore((s) => {
    const activeId = s.activeConversationId;
    return activeId ? s.streamStates[activeId]?.lastCompactSummary : undefined;
  });
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview);
  const clearIfConversationChanged = useGeneratedFilePreviewStore(
    (s) => s.clearIfConversationChanged,
  );
  const pushNotification = useNotificationStore((s) => s.push);

  // Sender identity for the chat row headers (avatar + name).
  // AI side follows the tenant brand (logoUrl + productName), so a custom
  // tenant logo / name automatically propagates into every chat. User side
  // falls back to the colored-initial ChatAvatar when no profile image is
  // configured (none of the current users have one).
  const assistantName = useBrandingStore((s) => s.productName);
  const assistantLogo = useBrandingStore((s) => s.logoUrl);
  const authUserName = useAuthStore(
    (s) => s.user?.name ?? s.user?.username ?? "我",
  );
  // In channel chats (WhatsApp/Telegram/dingtalk/...), the "user" role
  // bubbles come from the **external contact**, not the local AIjia operator.
  // Identity rules (2026-05-21):
  //   - WhatsApp: displayName (push_name) is reliable → use it for name +
  //     initial-style avatar so each contact looks distinct.
  //   - Other IM platforms (dingtalk/feishu/wecom/wechat/telegram): inbound
  //     messages don't carry a stable real name (feishu/wecom/wechat only
  //     give user_id). To stay visually consistent across IM tabs, render the
  //     platform display name + platform logo as the "from" side identity.
  //   - In-app (no channel binding): local auth user + neutral silhouette.
  const channelConversation = useChannelStore((s) => {
    if (!activeConversationId) return null;
    return (
      s.conversations.find((conv) => conv.sessionId === activeConversationId) ??
      null
    );
  });
  const { userName, userAvatarUrl, userAvatarVariant } = (() => {
    if (!channelConversation) {
      return {
        userName: authUserName,
        userAvatarUrl: null as string | null,
        userAvatarVariant: "neutral" as "initial" | "neutral",
      };
    }
    if (channelConversation.platform === "whatsapp") {
      const trimmed = channelConversation.displayName?.trim();
      return {
        userName: trimmed && trimmed.length > 0 ? trimmed : "WhatsApp 私聊",
        userAvatarUrl: null as string | null,
        userAvatarVariant: "initial" as "initial" | "neutral",
      };
    }
    return {
      userName:
        CHANNEL_PLATFORM_DISPLAY[channelConversation.platform] ??
        channelConversation.platform,
      userAvatarUrl: `/logos/${channelConversation.platform}.png`,
      userAvatarVariant: "initial" as "initial" | "neutral",
    };
  })();

  // Team chat drawer wiring.
  const { overview } = useTeamOverview(activeConversationId);
  const teamState = useConversationTeamState(activeConversationId);
  const openDrawer = useTeamStore((s) => s.openDrawer);
  const autoOpenedForConvRef = useRef<string | null>(null);

  // Auto-open the drawer the first time a team appears in the active
  // conversation, but only if the user hasn't manually closed it yet.
  // Re-armed when the active conversation changes.
  useEffect(() => {
    if (!activeConversationId) return;
    if (!overview || overview.teams.length === 0) return;
    if (autoOpenedForConvRef.current === activeConversationId) return;
    if (teamState.userClosedDrawer) {
      autoOpenedForConvRef.current = activeConversationId;
      return;
    }
    // Only auto-open while streaming — on conversation reload we leave it closed.
    if (!isStreaming) {
      autoOpenedForConvRef.current = activeConversationId;
      return;
    }
    openDrawer(activeConversationId);
    autoOpenedForConvRef.current = activeConversationId;
  }, [
    activeConversationId,
    overview,
    teamState.userClosedDrawer,
    isStreaming,
    openDrawer,
  ]);

  // Reset the auto-open guard when switching conversations.
  useEffect(() => {
    autoOpenedForConvRef.current = null;
  }, [activeConversationId]);

  // Walk turns in order; assign each TeamCreate marker to the next unused team session.
  // Team sessions on disk are ordered by createdAt; turns are ordered by message
  // chronology — so an ordinal pairing is correct (and is the same logic the
  // backend uses when grouping events into sessions).
  const teamSessionForTurnIdx = useMemo(() => {
    const result: Array<NonNullable<typeof overview>["teams"][number] | null> =
      [];
    if (!overview || overview.teams.length === 0) {
      return turns.map(() => null);
    }
    let teamCursor = 0;
    for (const t of turns) {
      if (
        t.teamMarker?.kind === "create" &&
        teamCursor < overview.teams.length
      ) {
        result.push(overview.teams[teamCursor]);
        teamCursor += 1;
      } else {
        result.push(null);
      }
    }
    return result;
  }, [turns, overview]);

  useEffect(() => {
    if (activeConversationId) clearIfConversationChanged(activeConversationId);
  }, [activeConversationId, clearIfConversationChanged]);

  const notifyFileError = (kind: FileActionKind, message: string) => {
    pushNotification({
      level: "error",
      title: FILE_ACTION_ERROR_TITLES[kind],
      message,
      actions: [],
      dismissible: true,
      context: "toast",
    });
  };

  const handlePreview = (file: RenderGeneratedFile) => {
    if (!file.conversationId) {
      notifyFileError("preview", "生成文件缺少所属对话，无法预览。");
      return;
    }
    openPreview(toPreviewTarget(file, file.conversationId));
  };

  const handleOpenExternal = async (file: RenderGeneratedFile) => {
    try {
      if (file.id.startsWith("artifact-") && file.filePath) {
        await openLocalFile(file.filePath);
        return;
      }
      if (!file.conversationId) {
        notifyFileError("open", "生成文件缺少所属对话，无法打开。");
        return;
      }
      await openGeneratedFile(file.id, file.conversationId);
    } catch (err) {
      notifyFileError(
        "open",
        err instanceof Error ? err.message : "打开生成文件失败。",
      );
    }
  };

  const handleDownload = async (file: RenderGeneratedFile) => {
    try {
      if (!file.conversationId) {
        notifyFileError("download", "生成文件缺少所属对话，无法下载。");
        return;
      }
      const savedPath = await savePreviewTargetToDisk(
        toPreviewTarget(file, file.conversationId),
      );
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
      notifyFileError(
        "download",
        err instanceof Error ? err.message : "下载生成文件失败。",
      );
    }
  };

  const handleReveal = async (file: RenderGeneratedFile) => {
    try {
      if (file.id.startsWith("artifact-") && file.filePath) {
        const parent = file.filePath.replace(/[/\\][^/\\]+$/, "") || "/";
        await openLocalFile(parent);
        return;
      }
      if (!file.conversationId) {
        notifyFileError("reveal", "生成文件缺少所属对话，无法定位。");
        return;
      }
      await revealFileInFolder(file.id, file.conversationId);
    } catch (err) {
      notifyFileError(
        "reveal",
        err instanceof Error ? err.message : "定位生成文件失败。",
      );
    }
  };

  const handleOpenTeamDrawer = (teamId: string) => {
    if (activeConversationId) openDrawer(activeConversationId, teamId);
  };

  const hasRenderedCompactBoundary = turns.some((turn) => turn.compactBoundary);
  return (
    <div
      className="flex flex-col gap-10 px-2 py-3"
      data-aijia-message-list
      data-aijia-streaming={isStreaming ? "true" : "false"}
    >
      {turns.map((t, i) => {
        if (t.compactBoundary) {
          return (
            <CompactBoundaryBar
              key={t.compactBoundary.id}
              preTokens={t.compactBoundary.preTokens}
              postTokens={t.compactBoundary.postTokens}
              tokensSaved={t.compactBoundary.tokensSaved}
              messagesSummarized={t.compactBoundary.messagesSummarized}
            />
          );
        }
        const teamSession = teamSessionForTurnIdx[i];
        // Employee dispatch prompts are represented by the chat top bar now
        // (employee avatar + identity + default skill), so the synthetic
        // user-message banner should not take space in the message stream.
        const isDispatchTurn = !!(
          t.userMessage && parseDispatchHeader(t.userMessage.text)
        );
        const aiAnchorIso = t.aiSegments[0]?.message.createdAt ?? null;
        return (
          <div key={i} className="flex flex-col gap-5">
            {t.peerBanners.length > 0 ? (
              <PeerMessageBanner banners={t.peerBanners} />
            ) : null}
            {t.userMessage ? (
              isDispatchTurn ? null : (
                <ChatRow
                  role="user"
                  name={userName}
                  avatarUrl={userAvatarUrl}
                  avatarVariant={userAvatarVariant}
                  timestamp={t.userMessage.createdAt}
                >
                  <UserMessageBubble
                    text={t.userMessage.text}
                    commandText={t.userMessage.commandText}
                    skillCommand={t.userMessage.skillCommand}
                    reasoningMode={t.userMessage.reasoningMode}
                    files={t.userMessage.files}
                    conversationId={activeConversationId ?? undefined}
                  />
                </ChatRow>
              )
            ) : null}
            {t.shouldCollapseCompletedProcess && t.completedFinalAnswer ? (
              renderCompletedFinalAnswerTurn(t.blocks, {
                assistantName,
                assistantLogo,
                aiAnchorIso,
                teamSession,
                expertTeam: expertTeam ?? null,
                onOpenTeamDrawer: handleOpenTeamDrawer,
                onPreview: handlePreview,
                onOpenExternal: handleOpenExternal,
                onDownload: handleDownload,
                onReveal: handleReveal,
                inlineStreamingContent: null,
                persistedBlockCount: t.persistedBlockCount ?? t.blocks.length,
                showFinalThinkingIndicator: false,
                finalAnswer: t.completedFinalAnswer,
                toolGroup: t.toolGroup,
              })
            ) : t.blocks && t.blocks.length > 0 ? (
              renderInterleavedBlocks(t.blocks, {
                assistantName,
                assistantLogo,
                aiAnchorIso,
                teamSession,
                expertTeam: expertTeam ?? null,
                onOpenTeamDrawer: handleOpenTeamDrawer,
                onPreview: handlePreview,
                onOpenExternal: handleOpenExternal,
                onDownload: handleDownload,
                onReveal: handleReveal,
                // Inline streamingContent only for the last turn while
                // active streaming is happening, so the bottom
                // StreamingBubble doesn't appear out of order below
                // live tool blocks.
                inlineStreamingContent:
                  i === turns.length - 1 &&
                  showStreamingBubble &&
                  streamingContent
                    ? streamingContent
                    : null,
                persistedBlockCount: t.persistedBlockCount ?? t.blocks.length,
                // 流式期间在 ChatRow 末尾挂一个 indicator-only 的 placeholder
                // StreamingBubble（content=""），用 absolute 渲染 typing 占位、
                // 不占 layout 高度。覆盖 waitingLlm / tools 阶段 streamingContent
                // 为空、但 turn 已有 blocks（导致 inline 路径和底部兜底都不
                // 渲染）这段窗口——保证整个 turn 期间 indicator 始终在。
                showFinalThinkingIndicator:
                  i === turns.length - 1 && showStreamingBubble,
              })
            ) : (
              // 兜底：turn 没有 blocks（罕见——测试 mock 或异常会话）。
              // 直接渲染 aiSegments / generatedFiles / suggestions / teamSession，
              // 不再尝试展示工具卡（没 blocks 时本来就没工具调用数据可显示）。
              <>
                {teamSession ? (
                  <TeamVisualProvider value={expertTeam ?? null}>
                    <TeamProgressBlock
                      session={teamSession}
                      onOpen={handleOpenTeamDrawer}
                    />
                  </TeamVisualProvider>
                ) : null}
                {t.aiSegments.length > 0 ||
                t.generatedFiles.length > 0 ||
                t.suggestions.length > 0 ? (
                  <ChatRow
                    role="assistant"
                    name={assistantName}
                    avatarUrl={assistantLogo}
                    timestamp={aiAnchorIso}
                  >
                    {t.aiSegments.map((s) => (
                      <AiBubble key={s.id} message={s.message} />
                    ))}
                    {t.generatedFiles.map((f) => (
                      <AvailableGeneratedFileCard
                        key={f.id}
                        file={f}
                        title={f.title}
                        sub={f.sub}
                        appName={
                          f.primaryAction === "preview" ? "预览" : f.appName
                        }
                        primaryAction={f.primaryAction}
                        canPreview={f.canPreview}
                        canOpenExternal={f.canOpenExternal}
                        canDownload
                        canReveal={f.canReveal}
                        filePath={f.filePath}
                        onPreview={() => handlePreview(f)}
                        onOpenExternal={() => void handleOpenExternal(f)}
                        onDownload={() => void handleDownload(f)}
                        onReveal={() => void handleReveal(f)}
                      />
                    ))}
                    {t.suggestions.length > 0 ? (
                      <SuggestChipGroup
                        items={t.suggestions.map((s) => ({
                          label: s,
                          onClick: () => {},
                        }))}
                      />
                    ) : null}
                  </ChatRow>
                ) : null}
              </>
            )}
          </div>
        );
      })}
      {!hasRenderedCompactBoundary && lastCompactSummary ? (
        <CompactBoundaryBar
          preTokens={lastCompactSummary.preTokens}
          postTokens={lastCompactSummary.postTokens}
          tokensSaved={lastCompactSummary.tokensSaved}
          messagesSummarized={lastCompactSummary.messagesSummarized}
        />
      ) : null}
      {(() => {
        if (!showStreamingBubble) return null;
        // Stream bubble 通常作为 inline 渲染在 last turn 的 ChatRow 内（紧贴
        // persisted blocks 末尾）。但如果 last turn 还没 blocks（第一个 iter、
        // 持久化未完成），inline 路径不会触发——这里兜底渲染一个底部 bubble，
        // 让用户仍能看到 live text。
        const lastTurn = turns[turns.length - 1];
        const inlineWillRender = lastTurn && (lastTurn.blocks?.length ?? 0) > 0;
        if (inlineWillRender) return null;
        return (
          <ChatRow
            role="assistant"
            name={assistantName}
            avatarUrl={assistantLogo}
          >
            <StreamingBubble content={streamingContent} />
          </ChatRow>
        );
      })()}
    </div>
  );

  /**
   * Render a turn's blocks in interleaved mode. The entire turn is wrapped
   * in a single `<ChatRow role="assistant">` so the avatar + name header
   * stamps exactly once per turn — regardless of whether the turn begins
   * with text or jumps straight into a tool call. Text bubbles and tool
   * cards then sit as siblings inside the row's content area in the natural
   * message order recorded in messages.jsonl.
   *
   * TeamProgressBlock follows the row, anchored to the team-create/delete
   * tool call's natural position at the end of the turn.
   */
  function renderInterleavedBlocks(
    blocks: RenderTurnBlock[],
    ctx: InterleavedRenderCtx,
  ) {
    const { children, firstTextIso } = buildInterleavedBlockNodes(blocks, ctx);

    // TeamProgressBlock 现在通过 'teamMarker' block 在 children 内联渲染（按
    // TeamCreate 在消息序列中的自然位置）。聚合模式下仍由 ChatRow 之前的
    // 独立分支渲染，与历史行为一致。
    return (
      <ChatRow
        role="assistant"
        name={ctx.assistantName}
        avatarUrl={ctx.assistantLogo}
        timestamp={firstTextIso ?? undefined}
      >
        {children}
      </ChatRow>
    );
  }

  type InterleavedRenderCtx = {
    assistantName: string;
    assistantLogo: string | null | undefined;
    aiAnchorIso: string | null;
    teamSession: NonNullable<typeof teamSessionForTurnIdx>[number];
    expertTeam: ReturnType<typeof getExpertTeam> | null;
    onOpenTeamDrawer: typeof handleOpenTeamDrawer;
    onPreview: (file: RenderGeneratedFile) => void | Promise<void>;
    onOpenExternal: (file: RenderGeneratedFile) => Promise<void>;
    onDownload: (file: RenderGeneratedFile) => Promise<void>;
    onReveal: (file: RenderGeneratedFile) => Promise<void>;
    /** Live text being streamed for the current iter (the next assistantText
     *  block that will be persisted). Rendered between persisted blocks
     *  and live tool blocks so the natural "text → tool" order is preserved. */
    inlineStreamingContent: string | null;
    /** Number of blocks at the start of `blocks` that come from persisted
     *  messages.jsonl. Blocks at >= this index are live toolExecutions. */
    persistedBlockCount: number;
    /** 流式期间在 children 末尾追加一个 indicator-only StreamingBubble
     *  （content=""），用 absolute 渲染 typing 占位，不占 layout 高度。 */
    showFinalThinkingIndicator: boolean;
  };

  function renderCompletedFinalAnswerTurn(
    blocks: RenderTurnBlock[],
    ctx: InterleavedRenderCtx & {
      finalAnswer: RenderAiSegment;
      toolGroup?: RenderToolGroup;
    },
  ) {
    const finalMessageId = ctx.finalAnswer.message.id;
    const finalAnswerIndex = blocks.findIndex(
      (block) =>
        block.kind === "assistantText" &&
        block.segment.message.id === finalMessageId,
    );
    const postFinalIndex =
      finalAnswerIndex >= 0
        ? blocks.findIndex(
            (block, index) =>
              index > finalAnswerIndex &&
              block.kind === "assistantText" &&
              block.segment.message.id !== finalMessageId,
          )
        : -1;
    const finalBlocks =
      finalAnswerIndex >= 0
        ? blocks.slice(
            finalAnswerIndex,
            postFinalIndex >= 0 ? postFinalIndex : blocks.length,
          )
        : [
            {
              kind: "assistantText" as const,
              id: ctx.finalAnswer.id,
              segment: ctx.finalAnswer,
            },
          ];
    const blocksBeforeFinal =
      finalAnswerIndex >= 0 ? blocks.slice(0, finalAnswerIndex) : blocks;
    const blocksAfterFinal =
      postFinalIndex >= 0 ? blocks.slice(postFinalIndex) : [];
    const visibleProcessSurfaceBlocks = blocksBeforeFinal.filter(
      (block) => block.kind === "teamMarker",
    );
    const processBlocks = blocksBeforeFinal.filter(
      (block) => block.kind !== "teamMarker",
    );
    const postFinalBlocks = blocksAfterFinal;
    const { children: processChildren, firstTextIso } =
      buildInterleavedBlockNodes(processBlocks, {
        ...ctx,
        inlineStreamingContent: null,
        persistedBlockCount: processBlocks.length,
        showFinalThinkingIndicator: false,
      });
    const { children: processSurfaceChildren } = buildInterleavedBlockNodes(
      visibleProcessSurfaceBlocks,
      {
        ...ctx,
        inlineStreamingContent: null,
        persistedBlockCount: visibleProcessSurfaceBlocks.length,
        showFinalThinkingIndicator: false,
      },
    );
    const { children: finalChildren } = buildInterleavedBlockNodes(finalBlocks, {
      ...ctx,
      inlineStreamingContent: null,
      persistedBlockCount: finalBlocks.length,
      showFinalThinkingIndicator: false,
    });
    const { children: postFinalChildren } = buildInterleavedBlockNodes(
      postFinalBlocks,
      {
        ...ctx,
        inlineStreamingContent: null,
        persistedBlockCount: postFinalBlocks.length,
        showFinalThinkingIndicator: false,
      },
    );
    const visibleProcessChildren = processChildren.filter(Boolean);
    const visibleProcessSurfaceChildren =
      processSurfaceChildren.filter(Boolean);
    const visibleFinalChildren = finalChildren.filter(Boolean);
    const visiblePostFinalChildren = postFinalChildren.filter(Boolean);

    return (
      <ChatRow
        role="assistant"
        name={ctx.assistantName}
        avatarUrl={ctx.assistantLogo}
        timestamp={firstTextIso ?? ctx.aiAnchorIso ?? undefined}
      >
        {visibleProcessChildren.length > 0 ? (
          <CompletedProcessCollapse toolGroup={ctx.toolGroup}>
            {visibleProcessChildren}
          </CompletedProcessCollapse>
        ) : null}
        {visibleProcessSurfaceChildren}
        {visibleFinalChildren}
        {visiblePostFinalChildren}
      </ChatRow>
    );
  }

  function buildInterleavedBlockNodes(
    blocks: RenderTurnBlock[],
    ctx: InterleavedRenderCtx,
  ): { children: ReactNode[]; firstTextIso: string | null } {
    const firstTextIso =
      blocks.find(
        (b): b is Extract<RenderTurnBlock, { kind: "assistantText" }> =>
          b.kind === "assistantText",
      )?.segment.message.createdAt ?? ctx.aiAnchorIso;

    const renderBlock = (b: RenderTurnBlock, idx: number) => {
      if (b.kind === "assistantText") {
        return <AiBubble key={b.id} message={b.segment.message} />;
      }
      if (b.kind === "generatedFile") {
        const f = b.file;
        return (
          <AvailableGeneratedFileCard
            key={f.id}
            file={f}
            title={f.title}
            sub={f.sub}
            appName={f.primaryAction === "preview" ? "预览" : f.appName}
            primaryAction={f.primaryAction}
            canPreview={f.canPreview}
            canOpenExternal={f.canOpenExternal}
            canDownload
            canReveal={f.canReveal}
            filePath={f.filePath}
            onPreview={() => ctx.onPreview(f)}
            onOpenExternal={() => void ctx.onOpenExternal(f)}
            onDownload={() => void ctx.onDownload(f)}
            onReveal={() => void ctx.onReveal(f)}
          />
        );
      }
      if (b.kind === "toolReceipt") {
        return (
          <ToolReceiptBlock key={b.id} receipt={b.receipt} step={b.step} />
        );
      }
      if (b.kind === "suggestions") {
        return (
          <SuggestChipGroup
            key={`sug-${idx}`}
            items={b.suggestions.map((s) => ({ label: s, onClick: () => {} }))}
          />
        );
      }
      if (b.kind === "teamMarker") {
        if (!ctx.teamSession) return null;
        return (
          <TeamVisualProvider
            key={`team-${b.toolCallId}`}
            value={ctx.expertTeam ?? null}
          >
            <TeamProgressBlock
              session={ctx.teamSession}
              onOpen={ctx.onOpenTeamDrawer}
            />
          </TeamVisualProvider>
        );
      }
      return null;
    };

    /** 扫一段连续 blocks：连续 toolStep 合并到一个 ToolStepGroupBlock；
     *  其他 block 各自走 renderBlock。`keyPrefix` 区分 persisted/live 两段，
     *  防止 React key 冲突；同时保证 persisted/live 分界不跨界合并 toolStep。 */
    const walkAndGroup = (
      slice: RenderTurnBlock[],
      keyPrefix: string,
      baseIdx: number,
    ): ReactNode[] => {
      const nodes: ReactNode[] = [];
      let pending: Array<Extract<RenderTurnBlock, { kind: "toolStep" }>> = [];
      const flush = () => {
        if (pending.length === 0) return;
        const firstId = pending[0]!.toolCallId;
        nodes.push(
          <ToolStepGroupBlock
            key={`${keyPrefix}-tg-${firstId}`}
            steps={pending.map((p) => p.step)}
          />,
        );
        pending = [];
      };
      slice.forEach((b, i) => {
        if (b.kind === "toolStep") {
          pending.push(b);
        } else {
          flush();
          nodes.push(renderBlock(b, baseIdx + i));
        }
      });
      flush();
      return nodes;
    };

    const splitAt = Math.min(ctx.persistedBlockCount, blocks.length);

    // 默认不按 turn 完成与否折叠"过程"——所有 blocks 按时序统一展开渲染。
    // 完成态折叠会把最终普通回复之前的过程 nodes 放进 CompletedProcessCollapse，
    // 并把最终普通回复单独展示在折叠过程之后。
    // - persisted 段（已落盘）走 walkAndGroup（连续 toolStep 合并）
    // - inline StreamingBubble 紧贴 persisted 渲染流式 text（suppressIndicator
    //   关掉自带 typing，避免和末尾 placeholder 重复）
    // - live 段（live toolStep）接在文字之后
    // - 末尾 indicator-only placeholder（content=""）兜底 typing，覆盖
    //   waitingLlm/tools 阶段 streamingContent 为空时的窗口
    const persistedNodes = walkAndGroup(
      blocks.slice(0, splitAt),
      "persisted",
      0,
    );
    const liveNodes = walkAndGroup(blocks.slice(splitAt), "live", splitAt);
    const children: ReactNode[] = [
      ...persistedNodes,
      ctx.inlineStreamingContent ? (
        <StreamingBubble
          key="streaming-inline"
          content={ctx.inlineStreamingContent}
          suppressIndicator
        />
      ) : null,
      ...liveNodes,
      ctx.showFinalThinkingIndicator ? (
        <StreamingBubble
          key="streaming-thinking"
          content=""
          treatAsHasContent
        />
      ) : null,
    ];
    return { children, firstTextIso };
  }
}
