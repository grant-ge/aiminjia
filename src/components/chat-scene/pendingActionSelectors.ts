import type { InteractionRequiredPayload, TurnStageKind } from "@/lib/tauri";
import type { PendingAsk } from "@/stores/streamingStore";

export type PendingPermissionAction = {
  kind: "permission";
  ask: PendingAsk;
};

export type PendingPermissionGroupAction = {
  kind: "permission-group";
  asks: PendingAsk[];
};

export type PendingUserQuestionAction = {
  kind: "user-question";
  interaction: InteractionRequiredPayload;
};

export type StalePermissionAction = {
  kind: "stale-permission";
  sessionId: string;
  toolName: string;
  toolCallId: string;
};

export type StaleInteractionAction = {
  kind: "stale-interaction";
  sessionId: string;
  interactionKind: string;
  interactionId: string;
};

export type PendingAction =
  | PendingPermissionAction
  | PendingPermissionGroupAction
  | PendingUserQuestionAction
  | StalePermissionAction
  | StaleInteractionAction;

function extractPermissionPath(message: string): string | null {
  const pathMatch = message.match(/(?:路径|path)\s*=\s*([^\n，。]+)/i);
  if (pathMatch?.[1]) return pathMatch[1].trim();
  const windowsPathMatch = message.match(/([A-Za-z]:[\\/][^\s，。?？]+)/);
  if (windowsPathMatch?.[1]) return windowsPathMatch[1].trim();
  const absolutePathMatch = message.match(/(\/[^\s，。?？]+)/);
  if (absolutePathMatch?.[1]) return absolutePathMatch[1].trim();
  return null;
}

function permissionParentPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const index = normalized.lastIndexOf("/");
  if (index <= 0) return normalized;
  return normalized.slice(0, index);
}

function permissionGroupKey(ask: PendingAsk): string {
  const target = extractPermissionPath(ask.message);
  const parent = target ? permissionParentPath(target) : ask.toolCallId;
  return [
    ask.conversationId,
    ask.runId,
    ask.mode,
    ask.rememberOptions?.join(",") ?? "",
    ask.defaultDestination ?? "",
    parent,
  ].join("\u0000");
}

function groupPermissionActions(activeAsks: PendingAsk[]): PendingAction[] {
  const groups = new Map<string, PendingAsk[]>();
  const order: string[] = [];

  activeAsks.forEach((ask) => {
    const key = permissionGroupKey(ask);
    if (!groups.has(key)) {
      groups.set(key, []);
      order.push(key);
    }
    groups.get(key)?.push(ask);
  });

  const actions: PendingAction[] = [];
  order.forEach((key) => {
    const asks = groups.get(key) ?? [];
    if (asks.length === 0) return;
    if (asks.length === 1) {
      actions.push({ kind: "permission", ask: asks[0] });
      return;
    }
    actions.push({ kind: "permission-group", asks });
  });
  return actions;
}

export function selectPendingActionForSession({
  sessionId,
  pendingAsks,
  pendingInteractions,
  turnStage,
  recoverableStalePermissionToolCallIds,
  recoverableStaleInteractionIds,
}: {
  sessionId: string | null;
  pendingAsks: Map<string, PendingAsk>;
  pendingInteractions: InteractionRequiredPayload[];
  turnStage?: TurnStageKind | null;
  recoverableStalePermissionToolCallIds?: ReadonlySet<string>;
  recoverableStaleInteractionIds?: ReadonlySet<string>;
}): PendingAction | null {
  return (
    selectPendingActionsForSession({
      sessionId,
      pendingAsks,
      pendingInteractions,
      turnStage,
      recoverableStalePermissionToolCallIds,
      recoverableStaleInteractionIds,
    })[0] ?? null
  );
}

export function selectPendingActionsForSession({
  sessionId,
  pendingAsks,
  pendingInteractions,
  turnStage,
  recoverableStalePermissionToolCallIds,
  recoverableStaleInteractionIds,
}: {
  sessionId: string | null;
  pendingAsks: Map<string, PendingAsk>;
  pendingInteractions: InteractionRequiredPayload[];
  turnStage?: TurnStageKind | null;
  recoverableStalePermissionToolCallIds?: ReadonlySet<string>;
  recoverableStaleInteractionIds?: ReadonlySet<string>;
}): PendingAction[] {
  if (!sessionId) return [];

  const actions: PendingAction[] = [];
  const activeAsks = Array.from(pendingAsks.values()).filter(
    (ask) => ask.conversationId === sessionId,
  );
  actions.push(...groupPermissionActions(activeAsks));

  if (
    turnStage?.kind === "waitingPermission" &&
    recoverableStalePermissionToolCallIds?.has(turnStage.toolCallId) === true &&
    !activeAsks.some((ask) => ask.toolCallId === turnStage.toolCallId)
  ) {
    actions.push({
      kind: "stale-permission",
      sessionId,
      toolName: turnStage.toolName,
      toolCallId: turnStage.toolCallId,
    });
  }

  const activeInteractions = pendingInteractions.filter(
    (interaction) => interaction.conversationId === sessionId,
  );
  actions.push(
    ...activeInteractions.map(
      (interaction): PendingAction => ({ kind: "user-question", interaction }),
    ),
  );

  if (
    turnStage?.kind === "waitingInteraction" &&
    recoverableStaleInteractionIds?.has(turnStage.interactionId) === true &&
    !activeInteractions.some(
      (interaction) => interaction.interactionId === turnStage.interactionId,
    )
  ) {
    actions.push({
      kind: "stale-interaction",
      sessionId,
      interactionKind: turnStage.interactionKind,
      interactionId: turnStage.interactionId,
    });
  }

  return actions;
}
