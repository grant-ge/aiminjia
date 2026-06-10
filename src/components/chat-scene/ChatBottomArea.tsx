/**
 * @designSource design.pen#Cbtm1 ChatBottomArea
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { SkillPopover } from "@/components/chat/SkillPopover";
import {
  RichComposer,
  pendingAttachmentsToTokens,
  useComposerAttachmentPaste,
  useComposerDropInbox,
  type RichComposerHandle,
  type RichComposerSubmitPayload,
} from "@/components/rich-composer";
import { useChat, type PendingFileInfo } from "@/hooks/useChat";
import { useChatAttachments } from "@/hooks/useChatAttachments";
import { useChatStore } from "@/stores/chatStore";
import { usePendingStore } from "@/stores/pendingStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useSkillStore } from "@/stores/skillStore";
import { useStreamingStore } from "@/stores/streamingStore";
import { useInteractionStore } from "@/stores/interactionStore";
import { useUiStore } from "@/stores/uiStore";
import {
  approvePermissionRequest,
  cancelPermissionRequest,
  cancelUserInteraction,
  clearActiveTurnStage,
  denyPermissionRequest,
  pendingInteractionSnapshotForSession,
  pendingPermissionSnapshotForSession,
  pendingSnapshotForSession,
  stopStreaming,
  submitUserInteraction,
} from "@/lib/tauri";
import { localizeSkill, localizedSkillName } from "@/lib/skillLocalization";
import { PendingChips } from "@/features/chat/PendingChips";
import { ensureExpertTeam } from "@/features/expert-teams/expertTeamRegistry";
import { getExpertTeam as findTeam } from "@/features/expert-teams/teams";
import { buildDirectorPrompt } from "@/features/expert-teams/buildDirectorPrompt";
import {
  PendingActionSurface,
  type PermissionDecision,
} from "./PendingActionSurface";
import { selectPendingActionsForSession } from "./pendingActionSelectors";

function BottomTips() {
  const { t } = useTranslation();
  return (
    <>
      <span>{t("bottomTips.aiDisclaimer")}</span>
      <div className="flex items-center gap-3">
        <span>{t("bottomTips.enterToSend")}</span>
        <span>{t("bottomTips.shiftEnterNewline")}</span>
        <span>{t("bottomTips.escToStop")}</span>
      </div>
    </>
  );
}

export function ChatBottomArea({
  disabled = false,
  sessionIdOverride,
  placeholderOverride,
}: {
  disabled?: boolean;
  /** When the bottom area is rendered inside a channel session view (DingTalk
   * etc.), the active session id does NOT live in chatStore — pass it
   * explicitly so pending chips / snapshot can target the right queue. */
  sessionIdOverride?: string;
  /** When set, overrides the default i18n placeholder. Used by expert-teams. */
  placeholderOverride?: string;
}) {
  const { t, i18n } = useTranslation();
  const composerRef = useRef<RichComposerHandle>(null);
  const activeConversationId = useChatStore((s) => s.activeConversationId);
  const messageCount = useChatStore((s) => s.messages.length);
  const pendingSessionId = sessionIdOverride ?? activeConversationId ?? null;
  const { sendUserMessage, isStreaming, stopCurrentStream } = useChat();
  const { isPickingAttachments, pickAttachments } = useChatAttachments();
  const [showSkillPopover, setShowSkillPopover] = useState(false);
  const skills = useSkillStore((s) => s.skills);
  const getSkillById = useSkillStore((s) => s.getById);
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? "full");
  const pendingAsks = useStreamingStore((s) => s.pendingAsks);
  const pendingTurnStage = useStreamingStore((s) =>
    pendingSessionId
      ? (s.streamStates[pendingSessionId]?.turnStage ?? null)
      : null,
  );
  const removePendingAsk = useStreamingStore((s) => s.removePendingAsk);
  const pendingInteractions = useInteractionStore((s) => s.pendingInteractions);
  const removeInteraction = useInteractionStore((s) => s.removeInteraction);
  const pendingActions = selectPendingActionsForSession({
    sessionId: pendingSessionId,
    pendingAsks,
    pendingInteractions,
    turnStage: pendingTurnStage,
  });
  // Snapshot of the installed skills as composer-friendly tokens.  The list
  // drives both the slash-command input rule inside the editor and the chip
  // rendered for any inline skill token already in the document.
  const skillTokens = useMemo(
    () =>
      skills.map((skill) => ({
        id: skill.id,
        label: localizeSkill(skill, i18n.language).name,
        command: skill.triggerText || `/${skill.id}`,
      })),
    [skills, i18n.language],
  );

  // One-shot prefill text (e.g., from generated suggestion); consumed synchronously
  // via lazy initializer so RichComposer's useEditor receives it on its very first render.
  const [initialMarkdown] = useState<string | undefined>(() => {
    const prefill = useUiStore.getState().consumePrefillText();
    return prefill ?? undefined;
  });

  useComposerDropInbox(composerRef);
  useComposerAttachmentPaste(composerRef);

  useEffect(() => {
    if (!isStreaming) {
      requestAnimationFrame(() => {
        composerRef.current?.focus();
      });
    }
  }, [activeConversationId, isStreaming]);

  useEffect(() => {
    if (!isStreaming) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      stopCurrentStream();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isStreaming, stopCurrentStream]);

  const handleSkillPick = useCallback(
    (skillId: string) => {
      const skill = getSkillById(skillId);
      composerRef.current?.insertSkillToken({
        id: skillId,
        label: localizedSkillName(skill, skillId, i18n.language),
        command: skill?.triggerText || `/${skillId}`,
      });
      composerRef.current?.focus();
      setShowSkillPopover(false);
    },
    [getSkillById, i18n.language],
  );

  const handleSubmit = useCallback(
    async (payload: RichComposerSubmitPayload) => {
      // Note: RichComposer.trySubmit already has a `submittingRef` guard against
      // duplicate concurrent calls for the same submission. We don't need a
      // separate `isSending` gate here, and adding one breaks the PendingQueue
      // UX: when the first message is in flight (sendUserMessage returns when
      // the IPC enqueues, but isSending was tied to LLM stream completion in
      // older code), a second Enter would silently drop.
      const fileInfos: PendingFileInfo[] = payload.attachments.map((f) => ({
        id: f.id,
        fileName: f.fileName,
        filePath: f.path,
        kind: f.kind,
        fileType: f.fileType,
        fileSize: f.fileSize,
        mimeType: f.mimeType,
      }));
      // The inline skill chip is the source of truth — it travels with the
      // doc, gets cleared automatically on submit, and is collected by the
      // serializer into payload.skills.  Only the first skill in a turn drives
      // the runtime; additional chips are dropped to avoid ambiguous routing.
      const skillForThisTurn = payload.skills[0] ?? null;
      let markdownToSend = payload.markdown;
      if (activeConversationId && messageCount === 0) {
        const teamId = await ensureExpertTeam(activeConversationId);
        const team = teamId ? findTeam(teamId, i18n.language) : undefined;
        if (team) {
          markdownToSend = buildDirectorPrompt(
            team,
            markdownToSend,
            i18n.language,
          );
        }
      }
      try {
        await sendUserMessage(
          markdownToSend,
          fileInfos.length > 0 ? fileInfos : undefined,
          skillForThisTurn,
        );
      } catch (err) {
        console.error("[ChatBottomArea] sendUserMessage failed:", err);
        throw err;
      }
    },
    [sendUserMessage, activeConversationId, messageCount, i18n.language],
  );

  const handlePickAttachments = useCallback(async () => {
    const results = await pickAttachments();
    if (results.length > 0) {
      composerRef.current?.insertAttachmentTokens(
        pendingAttachmentsToTokens(results),
      );
    }
  }, [pickAttachments]);

  const handleAllowPermission = useCallback(
    async (toolCallId: string, decision: PermissionDecision) => {
      const scopeKey = decision.remember
        ? decision.destination === "workspace"
          ? "pendingAction.permission.allowScopeWorkspace"
          : "pendingAction.permission.allowScopeUser"
        : "pendingAction.permission.allowScopeSession";
      const feedback = t("pendingAction.permission.allowedFeedback", {
        scope: t(scopeKey),
      });
      try {
        await approvePermissionRequest(
          toolCallId,
          null,
          decision.remember,
          decision.destination,
          feedback,
        );
        removePendingAsk(toolCallId);
      } catch (err) {
        console.error("[permission:ask] approve failed", err);
      }
    },
    [removePendingAsk, t],
  );

  const handleDenyPermission = useCallback(
    async (toolCallId: string, decision: PermissionDecision) => {
      try {
        await denyPermissionRequest(
          toolCallId,
          decision.feedback
            ? t("pendingAction.permission.deniedFeedbackWithUserInput", {
                feedback: decision.feedback,
              })
            : t("pendingAction.permission.deniedFeedback"),
          decision.remember,
          decision.destination,
        );
        removePendingAsk(toolCallId);
      } catch (err) {
        console.error("[permission:ask] deny failed", err);
      }
    },
    [removePendingAsk, t],
  );

  const handleCancelPermission = useCallback(
    async (toolCallId: string) => {
      try {
        await cancelPermissionRequest(
          toolCallId,
          t("pendingAction.permission.skippedFeedback"),
        );
        removePendingAsk(toolCallId);
      } catch (err) {
        console.error("[permission:ask] cancel failed", err);
      }
    },
    [removePendingAsk, t],
  );

  const handleSubmitInteraction = useCallback(
    async (
      interactionId: string,
      value: { answers: Record<string, string> },
    ) => {
      try {
        await submitUserInteraction(interactionId, value);
        removeInteraction(interactionId);
      } catch (err) {
        console.error("[interaction:required] submit failed", err);
      }
    },
    [removeInteraction],
  );

  const handleCancelInteraction = useCallback(
    async (interactionId: string) => {
      try {
        await cancelUserInteraction(
          interactionId,
          t("pendingAction.interaction.cancelledFeedback"),
        );
        removeInteraction(interactionId);
      } catch (err) {
        console.error("[interaction:required] cancel failed", err);
      }
    },
    [removeInteraction, t],
  );

  const handleClearStalePermission = useCallback(async (sessionId: string) => {
    try {
      await stopStreaming(sessionId);
    } catch (err) {
      console.error("[permission:stale] stop failed", err);
    }
    try {
      await clearActiveTurnStage(sessionId);
      useStreamingStore.getState().clearConversationStreamState(sessionId);
    } catch (err) {
      console.error("[permission:stale] clear failed", err);
    }
  }, []);

  const handleClearStaleInteraction = useCallback(async (sessionId: string) => {
    try {
      await stopStreaming(sessionId);
    } catch (err) {
      console.error("[interaction:stale] stop failed", err);
    }
    try {
      await clearActiveTurnStage(sessionId);
      useStreamingStore.getState().clearConversationStreamState(sessionId);
    } catch (err) {
      console.error("[interaction:stale] clear failed", err);
    }
  }, []);

  // Fetch pending queue snapshot when conversation switches.
  // Backend pushes incremental updates via pending:queued/drained/removed events.
  useEffect(() => {
    if (!pendingSessionId) return;
    pendingSnapshotForSession(pendingSessionId)
      .then((items) =>
        usePendingStore.getState().applySnapshot(pendingSessionId, items),
      )
      .catch((e) => {
        // eslint-disable-next-line no-console
        console.warn("[pending] snapshot fetch failed", e);
      });
  }, [pendingSessionId]);

  // Fetch active permission asks when switching conversations. The live
  // `permission:ask` event can be missed after reload / listener remount, while
  // the persisted turn stage still shows "waitingPermission" in the transcript.
  // Snapshotting from the backend keeps the composer intercepted in that case.
  // Keep this non-destructive: an empty or lagging snapshot must not erase a
  // live permission ask that just arrived through the event stream.
  useEffect(() => {
    if (!pendingSessionId) return;
    pendingPermissionSnapshotForSession(pendingSessionId)
      .then((asks) => {
        const store = useStreamingStore.getState();
        asks.forEach((ask) => store.addPendingAsk(ask));
      })
      .catch((e) => {
        // eslint-disable-next-line no-console
        console.warn("[permission] snapshot fetch failed", e);
      });
  }, [pendingSessionId]);

  // Fetch active user interactions when switching conversations. The runtime
  // can still be awaiting AskUserQuestion after HMR/reload even though the
  // frontend interaction store was recreated.
  useEffect(() => {
    if (!pendingSessionId) return;
    pendingInteractionSnapshotForSession(pendingSessionId)
      .then((interactions) => {
        const store = useInteractionStore.getState();
        interactions.forEach((interaction) =>
          store.addInteraction(interaction),
        );
      })
      .catch((e) => {
        // eslint-disable-next-line no-console
        console.warn("[interaction] snapshot fetch failed", e);
      });
  }, [pendingSessionId]);

  return (
    <footer data-testid="chat-bottom-area" className="relative shrink-0">
      <div className="px-6 pb-4 pt-3">
        <div
          data-testid="chat-composer-width-shell"
          className={
            chatWidthMode === "full"
              ? "relative w-full"
              : "relative mx-auto w-full max-w-[736px]"
          }
        >
          <div className="absolute bottom-full left-1/2 z-30 mb-1 -translate-x-1/2">
            <SkillPopover
              open={showSkillPopover}
              onPick={handleSkillPick}
              onClose={() => setShowSkillPopover(false)}
            />
          </div>

          <div className="relative">
            {pendingSessionId && <PendingChips sessionId={pendingSessionId} />}
            {pendingActions.length > 0 ? (
              <PendingActionSurface
                action={pendingActions}
                onAllowPermission={handleAllowPermission}
                onDenyPermission={handleDenyPermission}
                onCancelPermission={handleCancelPermission}
                onSubmitInteraction={handleSubmitInteraction}
                onCancelInteraction={handleCancelInteraction}
                onClearStalePermission={handleClearStalePermission}
                onClearStaleInteraction={handleClearStaleInteraction}
              />
            ) : (
              <RichComposer
                ref={composerRef}
                placeholder={placeholderOverride ?? t("inputBar.placeholder")}
                onSubmit={handleSubmit}
                disabled={disabled}
                isStreaming={isStreaming}
                onStop={stopCurrentStream}
                clearOnSubmit
                autoFocus
                initialMarkdown={initialMarkdown}
                showProjectButton={false}
                onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
                skillTokens={skillTokens}
                onOpenAttachment={
                  isPickingAttachments
                    ? undefined
                    : () => void handlePickAttachments()
                }
                tips={<BottomTips />}
                containerClassName="[border-color:var(--composer-border)] shadow-[var(--shadow-composer)]"
                limitEditorHeight
              />
            )}
          </div>
        </div>
      </div>
    </footer>
  );
}
