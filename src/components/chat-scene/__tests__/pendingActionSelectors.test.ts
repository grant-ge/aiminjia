import { describe, expect, it } from "vitest";
import type { InteractionRequiredPayload, TurnStageKind } from "@/lib/tauri";
import type { PendingAsk } from "@/stores/streamingStore";
import {
  selectPendingActionForSession,
  selectPendingActionsForSession,
  type PendingAction,
} from "../pendingActionSelectors";

function ask(overrides: Partial<PendingAsk> = {}): PendingAsk {
  return {
    conversationId: "conv-1",
    runId: "run-1",
    toolCallId: "tool-1",
    toolName: "Read",
    message: "Read local file?",
    suggestions: ["allow", "deny"],
    mode: "default",
    rememberOptions: ["session", "workspace"],
    defaultDestination: "session",
    ...overrides,
  };
}

function interaction(
  overrides: Partial<InteractionRequiredPayload> = {},
): InteractionRequiredPayload {
  return {
    conversationId: "conv-1",
    runId: "run-1",
    interactionId: "ask-1",
    toolCallId: "tool-1",
    toolName: "AskUser",
    kind: "askUserQuestion",
    payload: {
      questions: [
        {
          question: "Pick one",
          header: "Pick",
          options: [
            { label: "A", description: "A path" },
            { label: "B", description: "B path" },
          ],
        },
      ],
    },
    ...overrides,
  };
}

describe("selectPendingActionForSession", () => {
  it("returns null when no session is active", () => {
    const result = selectPendingActionForSession({
      sessionId: null,
      pendingAsks: new Map([["tool-1", ask()]]),
      pendingInteractions: [interaction()],
    });

    expect(result).toBeNull();
  });

  it("selects the permission ask for the active conversation", () => {
    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map([
        [
          "tool-other",
          ask({ conversationId: "conv-2", toolCallId: "tool-other" }),
        ],
        ["tool-1", ask()],
      ]),
      pendingInteractions: [],
    });

    expect(result).toEqual<PendingAction>({
      kind: "permission",
      ask: ask(),
    });
  });

  it("does not select a pending ask from another conversation", () => {
    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map([
        [
          "tool-other",
          ask({ conversationId: "conv-2", toolCallId: "tool-other" }),
        ],
      ]),
      pendingInteractions: [],
    });

    expect(result).toBeNull();
  });

  it("selects AskUserQuestion when there is no active permission ask", () => {
    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map(),
      pendingInteractions: [interaction()],
    });

    expect(result).toEqual<PendingAction>({
      kind: "user-question",
      interaction: interaction(),
    });
  });

  it("does not select AskUserQuestion from another conversation", () => {
    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map(),
      pendingInteractions: [
        interaction({ conversationId: "conv-2", interactionId: "ask-other" }),
      ],
    });

    expect(result).toBeNull();
  });

  it("prioritizes permission over AskUserQuestion for the same conversation", () => {
    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map([["tool-1", ask()]]),
      pendingInteractions: [interaction()],
      turnStage: {
        kind: "waitingPermission",
        toolName: "Glob",
        toolCallId: "tool-stage",
      },
    });

    expect(result?.kind).toBe("permission");
  });

  it("returns all live pending actions for the active conversation in priority order", () => {
    const result = selectPendingActionsForSession({
      sessionId: "conv-1",
      pendingAsks: new Map([
        [
          "tool-other",
          ask({ conversationId: "conv-2", toolCallId: "tool-other" }),
        ],
        ["tool-1", ask()],
      ]),
      pendingInteractions: [
        interaction({ conversationId: "conv-2", interactionId: "ask-other" }),
        interaction(),
      ],
    });

    expect(result).toEqual<PendingAction[]>([
      { kind: "permission", ask: ask() },
      { kind: "user-question", interaction: interaction() },
    ]);
  });

  it("does not let a stale permission stage hide a live AskUserQuestion", () => {
    const result = selectPendingActionsForSession({
      sessionId: "conv-1",
      pendingAsks: new Map(),
      pendingInteractions: [interaction()],
      turnStage: {
        kind: "waitingPermission",
        toolName: "Bash",
        toolCallId: "stale-bash-call",
      },
    });

    expect(result).toEqual<PendingAction[]>([
      { kind: "user-question", interaction: interaction() },
    ]);
  });

  it("groups related permission asks from the same run and directory", () => {
    const result = selectPendingActionsForSession({
      sessionId: "conv-1",
      pendingAsks: new Map([
        [
          "tool-1",
          ask({
            toolCallId: "tool-1",
            toolName: "Read",
            message:
              "该路径未授权，需要用户确认：路径=/private/tmp/aijia/one.txt",
          }),
        ],
        [
          "tool-2",
          ask({
            toolCallId: "tool-2",
            toolName: "Read",
            message:
              "该路径未授权，需要用户确认：路径=/private/tmp/aijia/two.txt",
          }),
        ],
        [
          "tool-3",
          ask({
            toolCallId: "tool-3",
            toolName: "Read",
            message: "该路径未授权，需要用户确认：路径=/private/tmp/other.txt",
          }),
        ],
      ]),
      pendingInteractions: [],
    });

    expect(result).toEqual<PendingAction[]>([
      {
        kind: "permission-group",
        asks: [
          ask({
            toolCallId: "tool-1",
            toolName: "Read",
            message:
              "该路径未授权，需要用户确认：路径=/private/tmp/aijia/one.txt",
          }),
          ask({
            toolCallId: "tool-2",
            toolName: "Read",
            message:
              "该路径未授权，需要用户确认：路径=/private/tmp/aijia/two.txt",
          }),
        ],
      },
      {
        kind: "permission",
        ask: ask({
          toolCallId: "tool-3",
          toolName: "Read",
          message: "该路径未授权，需要用户确认：路径=/private/tmp/other.txt",
        }),
      },
    ]);
  });

  it("groups same-scope permission asks across read tools", () => {
    const result = selectPendingActionsForSession({
      sessionId: "conv-1",
      pendingAsks: new Map([
        [
          "tool-1",
          ask({
            toolCallId: "tool-1",
            toolName: "Grep",
            message: "该路径未授权，需要用户确认：路径=/private/tmp",
          }),
        ],
        [
          "tool-2",
          ask({
            toolCallId: "tool-2",
            toolName: "Glob",
            message: "该路径未授权，需要用户确认：路径=/private/tmp",
          }),
        ],
      ]),
      pendingInteractions: [],
    });

    expect(result).toEqual<PendingAction[]>([
      {
        kind: "permission-group",
        asks: [
          ask({
            toolCallId: "tool-1",
            toolName: "Grep",
            message: "该路径未授权，需要用户确认：路径=/private/tmp",
          }),
          ask({
            toolCallId: "tool-2",
            toolName: "Glob",
            message: "该路径未授权，需要用户确认：路径=/private/tmp",
          }),
        ],
      },
    ]);
  });

  it("does not surface persisted waitingPermission stage when no live ask exists", () => {
    const stage: TurnStageKind = {
      kind: "waitingPermission",
      toolName: "Glob",
      toolCallId: "tool-stage",
    };

    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map(),
      pendingInteractions: [],
      turnStage: stage,
    });

    expect(result).toBeNull();
  });

  it("does not surface persisted waitingInteraction stage when no live interaction exists", () => {
    const stage: TurnStageKind = {
      kind: "waitingInteraction",
      interactionKind: "askUserQuestion",
      interactionId: "interaction-stage",
    };

    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map(),
      pendingInteractions: [],
      turnStage: stage,
    });

    expect(result).toBeNull();
  });

  it("does not recover a stale permission action for non-waiting stages", () => {
    const result = selectPendingActionForSession({
      sessionId: "conv-1",
      pendingAsks: new Map(),
      pendingInteractions: [],
      turnStage: { kind: "waitingLlm", iteration: 0 },
    });

    expect(result).toBeNull();
  });
});
