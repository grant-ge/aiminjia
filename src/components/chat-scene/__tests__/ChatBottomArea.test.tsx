import "@testing-library/jest-dom";
import { describe, expect, it, vi, beforeEach } from "vitest";
import {
  render,
  waitFor,
  fireEvent,
  act,
  screen,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChatBottomArea } from "../ChatBottomArea";
import { useChatStore } from "@/stores/chatStore";
import { useStreamingStore, type PendingAsk } from "@/stores/streamingStore";
import { useInteractionStore } from "@/stores/interactionStore";
import { useSkillStore } from "@/stores/skillStore";
import type { InteractionRequiredPayload } from "@/lib/tauri";

const tauriMocks = vi.hoisted(() => ({
  pendingSnapshotForSession: vi.fn(),
  pendingPermissionSnapshotForSession: vi.fn(),
  pendingInteractionSnapshotForSession: vi.fn(),
  clearActiveTurnStage: vi.fn(),
  stopStreaming: vi.fn(),
  approvePermissionRequest: vi.fn(),
  denyPermissionRequest: vi.fn(),
  cancelPermissionRequest: vi.fn(),
  submitUserInteraction: vi.fn(),
  cancelUserInteraction: vi.fn(),
}));

const i18nMock = vi.hoisted(() => ({
  language: "zh-CN",
  translations: {
    "zh-CN": {
      "pendingAction.permission.title":
        "需要你允许我用提升权限只读检查 {{target}} 是否可访问。",
      "pendingAction.permission.allowOnce": "是",
      "pendingAction.permission.allowRemember":
        "是，且对于后续同类操作不再询问",
      "pendingAction.permission.deny": "否，请告知如何调整",
      "pendingAction.permission.denyPlaceholder":
        "例如：不要读取这个目录，改用工作区里的摘要文件。",
      "pendingAction.permission.skip": "拒绝",
      "pendingAction.permission.submit": "提交",
      "pendingAction.permission.allowedFeedback":
        "用户允许了这个权限申请（{{scope}}）。",
      "pendingAction.permission.allowScopeSession": "仅本次",
      "pendingAction.permission.allowScopeWorkspace":
        "后续同工作区同类操作不再询问",
      "pendingAction.permission.allowScopeUser": "后续同用户同类操作不再询问",
      "pendingAction.permission.deniedFeedback":
        "用户拒绝了这个权限申请",
      "pendingAction.permission.deniedFeedbackWithUserInput":
        "用户拒绝了这个权限申请，并给出调整说明：{{feedback}}",
      "pendingAction.permission.skippedFeedback":
        "用户跳过了这个权限申请，当前任务已停止。",
      "pendingAction.interaction.title": "需要补充信息",
      "pendingAction.interaction.previous": "上一题",
      "pendingAction.interaction.next": "下一题",
      "pendingAction.interaction.progress": "{{current}} of {{total}}",
      "pendingAction.interaction.skip": "忽略",
      "pendingAction.interaction.continue": "继续",
      "pendingAction.interaction.stop": "停止任务",
      "pendingAction.interaction.custom": "其他",
      "pendingAction.interaction.customPlaceholder": "请输入你的调整说明",
      "pendingAction.interaction.submittedFeedback":
        "用户回答了补充问题：{{summary}}",
      "pendingAction.interaction.cancelledFeedback":
        "用户忽略了这个补充问题。请基于已有信息继续；不要输出空标题、空书名号或空占位符，缺少关键名称时请使用通用名称。",
      "pendingAction.stalePermission.title": "需要恢复任务状态",
      "pendingAction.stalePermission.messageBefore":
        "上一次任务停在权限确认阶段：",
      "pendingAction.stalePermission.messageAfter":
        "。当前运行时没有找到可继续处理的权限申请，请停止当前任务后重新发送。",
      "pendingAction.stalePermission.stop": "停止任务",
      "pendingAction.staleInteraction.title": "需要恢复任务状态",
      "pendingAction.staleInteraction.messageBefore":
        "上一次任务停在补充问题阶段：",
      "pendingAction.staleInteraction.messageAfter":
        "。当前运行时没有找到可继续处理的问题，请停止当前任务后重新发送。",
      "pendingAction.staleInteraction.stop": "停止任务",
    },
    "en-US": {
      "pendingAction.permission.allowedFeedback":
        "The user approved this permission request ({{scope}}).",
      "pendingAction.permission.allowScopeSession": "this time only",
      "pendingAction.permission.allowScopeWorkspace":
        "do not ask again for similar operations in this workspace",
      "pendingAction.permission.allowScopeUser":
        "do not ask again for similar operations for this user",
      "pendingAction.permission.deniedFeedback":
        "The user denied this permission request.",
      "pendingAction.permission.deniedFeedbackWithUserInput":
        "The user denied this permission request and provided adjustment guidance: {{feedback}}",
      "pendingAction.permission.skippedFeedback":
        "The user skipped this permission request. The current task has been stopped.",
      "pendingAction.interaction.custom": "Other",
      "pendingAction.interaction.customPlaceholder": "Enter your adjustment",
      "pendingAction.interaction.submittedFeedback":
        "The user answered the follow-up question: {{summary}}",
      "pendingAction.interaction.cancelledFeedback":
        "The user ignored this follow-up question. Continue with the available information; do not output empty titles, empty quoted names, or empty placeholders. Use a generic name when a required name is missing.",
    },
  },
}));

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    pendingSnapshotForSession: tauriMocks.pendingSnapshotForSession,
    pendingPermissionSnapshotForSession:
      tauriMocks.pendingPermissionSnapshotForSession,
    pendingInteractionSnapshotForSession:
      tauriMocks.pendingInteractionSnapshotForSession,
    clearActiveTurnStage: tauriMocks.clearActiveTurnStage,
    stopStreaming: tauriMocks.stopStreaming,
    approvePermissionRequest: tauriMocks.approvePermissionRequest,
    denyPermissionRequest: tauriMocks.denyPermissionRequest,
    cancelPermissionRequest: tauriMocks.cancelPermissionRequest,
    submitUserInteraction: tauriMocks.submitUserInteraction,
    cancelUserInteraction: tauriMocks.cancelUserInteraction,
  };
});

vi.mock("@tiptap/react", async (importOriginal) => {
  const mod = await importOriginal<typeof import("@tiptap/react")>();
  return { ...mod, ReactNodeViewRenderer: () => () => ({}) };
});

const mockSendUserMessage = vi.fn();
const mockStopCurrentStream = vi.fn();
let mockIsStreaming = false;
const mockPickAttachments = vi.fn();

vi.mock("@/hooks/useChat", () => ({
  useChat: () => ({
    sendUserMessage: mockSendUserMessage,
    isStreaming: mockIsStreaming,
    stopCurrentStream: mockStopCurrentStream,
  }),
}));

vi.mock("@/hooks/useChatAttachments", () => ({
  useChatAttachments: () => ({
    isPickingAttachments: false,
    pickAttachments: mockPickAttachments,
    saveClipboardImage: vi.fn(),
    resolvePastedPaths: vi.fn(),
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string | number>) => {
      const translations = i18nMock.translations[
        i18nMock.language as "zh-CN" | "en-US"
      ] as Record<string, string>;
      const template = translations[key] ?? key;
      return Object.entries(values ?? {}).reduce(
        (text, [name, value]) => text.replaceAll(`{{${name}}}`, String(value)),
        template,
      );
    },
    i18n: {
      get language() {
        return i18nMock.language;
      },
    },
  }),
}));

beforeEach(() => {
  i18nMock.language = "zh-CN";
  mockSendUserMessage.mockReset().mockResolvedValue(undefined);
  mockStopCurrentStream.mockReset();
  mockPickAttachments.mockReset().mockResolvedValue([]);
  tauriMocks.pendingSnapshotForSession.mockReset().mockResolvedValue([]);
  tauriMocks.pendingPermissionSnapshotForSession
    .mockReset()
    .mockResolvedValue([]);
  tauriMocks.pendingInteractionSnapshotForSession
    .mockReset()
    .mockResolvedValue([]);
  tauriMocks.clearActiveTurnStage.mockReset().mockResolvedValue(undefined);
  tauriMocks.stopStreaming.mockReset().mockResolvedValue(undefined);
  tauriMocks.approvePermissionRequest.mockReset().mockResolvedValue(undefined);
  tauriMocks.denyPermissionRequest.mockReset().mockResolvedValue(undefined);
  tauriMocks.cancelPermissionRequest.mockReset().mockResolvedValue(undefined);
  tauriMocks.submitUserInteraction.mockReset().mockResolvedValue(undefined);
  tauriMocks.cancelUserInteraction.mockReset().mockResolvedValue(undefined);
  mockIsStreaming = false;
  useChatStore.setState({
    activeConversationId: "conv-1",
    messages: [],
  });
  useStreamingStore.setState({ pendingAsks: new Map() });
  useInteractionStore.setState({ pendingInteractions: [] });
  useSkillStore.setState({
    skills: [
      {
        id: "dingtalk-workspace",
        displayName: "玩转钉钉",
        displayNameEn: "DingTalk Workspace",
        description: "desc",
        source: "global",
        hasWorkflow: false,
        icon: "",
        shortDescription: "desc",
        shortDescriptionEn: "desc",
        triggerText: "/dingtalk-workspace",
        category: "general",
        updatedAt: null,
      },
    ],
  });
});

function pendingAsk(overrides: Partial<PendingAsk> = {}): PendingAsk {
  return {
    conversationId: "conv-1",
    runId: "run-1",
    toolCallId: "tool-1",
    toolName: "Read",
    message: "该路径未授权，需要用户确认：路径=/tmp/a.txt",
    suggestions: [],
    mode: "default",
    rememberOptions: ["session"],
    defaultDestination: "session",
    ...overrides,
  };
}

function pendingInteraction(
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
          header: "Input",
          question: "Need input?",
          options: [{ label: "Yes", description: "Continue" }],
        },
      ],
    },
    ...overrides,
  };
}

describe("ChatBottomArea", () => {
  it("renders RichComposer", async () => {
    render(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
  });

  it("uses the crisp chat composer border and shadow tokens", async () => {
    const { container } = render(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );

    const composer = container.querySelector(
      '[data-testid="composer-root"]',
    ) as HTMLElement;
    expect(composer).toHaveClass("[border-color:var(--composer-border)]");
    expect(composer).toHaveClass("shadow-[var(--shadow-composer)]");
    expect(composer).not.toHaveClass("shadow-[var(--shadow-md)]");
  });

  it("typing + Enter calls sendUserMessage with markdown text and no attachments", async () => {
    const user = userEvent.setup();
    render(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
    const editor = document.querySelector(".ProseMirror") as HTMLElement;
    await user.click(editor);
    await user.type(editor, "hello");
    fireEvent.keyDown(editor, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1));
    expect(mockSendUserMessage.mock.calls[0][0]).toBe("hello");
    expect(mockSendUserMessage.mock.calls[0][1]).toBeUndefined();
  });

  it("attachment-only Enter sends markdown with file:// link and attachment array", async () => {
    mockPickAttachments.mockResolvedValueOnce([
      {
        id: "a",
        fileName: "a.pdf",
        path: "/p/a.pdf",
        kind: "file",
        fileType: "pdf",
        fileSize: 0,
        mimeType: undefined,
        source: "picker",
      },
    ]);
    const { container } = render(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
    const attachBtn = container.querySelector(
      '[aria-label="composer.addAttachment"]',
    ) as HTMLElement;
    await act(async () => {
      attachBtn.click();
    });
    await waitFor(() => {
      const html = document.querySelector(".ProseMirror")?.innerHTML ?? "";
      expect(html).toContain("a.pdf");
    });
    const editor = document.querySelector(".ProseMirror") as HTMLElement;
    fireEvent.keyDown(editor, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalled());
    const [text, files] = mockSendUserMessage.mock.calls[0];
    expect(text).toContain("[附件: a.pdf](<file:///p/a.pdf>)");
    expect(files).toHaveLength(1);
    expect(files[0].id).toBe("a");
  });

  it("isStreaming → shows stop button, click calls stopCurrentStream", async () => {
    mockIsStreaming = true;
    const { container } = render(<ChatBottomArea />);
    const stopBtn = await waitFor(
      () =>
        container.querySelector('[aria-label="composer.stop"]') as HTMLElement,
    );
    fireEvent.click(stopBtn);
    expect(mockStopCurrentStream).toHaveBeenCalledTimes(1);
    expect(mockSendUserMessage).not.toHaveBeenCalled();
  });

  it("isStreaming → Escape calls stopCurrentStream", async () => {
    mockIsStreaming = true;
    render(<ChatBottomArea />);

    fireEvent.keyDown(window, { key: "Escape", code: "Escape" });

    expect(mockStopCurrentStream).toHaveBeenCalledTimes(1);
    expect(mockSendUserMessage).not.toHaveBeenCalled();
  });

  it("renders the Escape stop shortcut tip", () => {
    render(<ChatBottomArea />);

    expect(screen.getByText("bottomTips.escToStop")).toBeInTheDocument();
  });

  it("picking a skill inserts inline token and submit passes skill metadata", async () => {
    const user = userEvent.setup();
    const { container } = render(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
    const skillButton = container.querySelector(
      '[aria-label="composer.openSkillPicker"]',
    ) as HTMLElement;
    await user.click(skillButton);
    await user.click(await screen.findByText("玩转钉钉"));
    const editor = document.querySelector(".ProseMirror") as HTMLElement;
    expect(editor.textContent).toContain("玩转钉钉");
    expect(editor.textContent).not.toContain("/dingtalk-workspace");
    await user.click(editor);
    await user.type(editor, " 查今天日程");
    fireEvent.keyDown(editor, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(mockSendUserMessage).toHaveBeenCalledTimes(1));
    expect(mockSendUserMessage.mock.calls[0][0]).toBe(" 查今天日程");
    expect(mockSendUserMessage.mock.calls[0][2]).toEqual({
      id: "dingtalk-workspace",
      label: "玩转钉钉",
      command: "/dingtalk-workspace",
    });
  });

  it("empty Enter does not call sendUserMessage", async () => {
    render(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
    const editor = document.querySelector(".ProseMirror") as HTMLElement;
    fireEvent.keyDown(editor, { key: "Enter", code: "Enter" });
    expect(mockSendUserMessage).not.toHaveBeenCalled();
  });

  it("replaces the composer with permission surface for the active conversation", async () => {
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    render(<ChatBottomArea />);

    expect(
      await screen.findByText(/需要你允许我用提升权限只读检查/),
    ).toBeInTheDocument();
    expect(document.querySelector(".ProseMirror")).toBeNull();
  });

  it("does not replace the composer for another conversation pending ask", async () => {
    useStreamingStore.getState().addPendingAsk(
      pendingAsk({
        conversationId: "conv-2",
        runId: "run-2",
        toolCallId: "tool-2",
        message: "Other conversation",
      }),
    );

    render(<ChatBottomArea />);

    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
    expect(
      screen.queryByText(/需要你允许我用提升权限只读检查/),
    ).not.toBeInTheDocument();
  });

  it("restores the permission surface after switching back to the pending conversation", async () => {
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    const { rerender } = render(<ChatBottomArea />);
    expect(
      await screen.findByText(/需要你允许我用提升权限只读检查/),
    ).toBeInTheDocument();

    act(() => {
      useChatStore.setState({ activeConversationId: "conv-2" });
    });
    rerender(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );

    act(() => {
      useChatStore.setState({ activeConversationId: "conv-1" });
    });
    rerender(<ChatBottomArea />);
    expect(
      await screen.findByText(/需要你允许我用提升权限只读检查/),
    ).toBeInTheDocument();
  });

  it("restores the permission surface from backend snapshot when the live ask event was missed", async () => {
    tauriMocks.pendingPermissionSnapshotForSession.mockResolvedValueOnce([
      pendingAsk({
        toolCallId: "tool-glob",
        toolName: "Glob",
        message: "该路径未授权，需要用户确认：路径=/Users/oayzz/.renlijia",
      }),
    ]);

    render(<ChatBottomArea />);

    expect(
      await screen.findByText(/需要你允许我用提升权限只读检查/),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText(/\/Users\/oayzz\/\.renlijia/).length,
    ).toBeGreaterThan(0);
    expect(document.querySelector(".ProseMirror")).toBeNull();
    expect(tauriMocks.pendingPermissionSnapshotForSession).toHaveBeenCalledWith(
      "conv-1",
    );
  });

  it("intercepts the composer from persisted waitingPermission stage when no pending ask survived reload", async () => {
    const user = userEvent.setup();
    useStreamingStore.setState({
      pendingAsks: new Map(),
      streamStates: {
        "conv-1": {
          isStreaming: false,
          streamingContent: "",
          toolExecutions: [],
          turnStage: {
            kind: "waitingPermission",
            toolName: "Glob",
            toolCallId: "tool-stage",
          },
          stageStartedAt: Date.now(),
          lastHeartbeatAt: Date.now(),
          turnStartedAt: Date.now(),
        },
      },
    });

    render(<ChatBottomArea />);

    expect(await screen.findByText("需要恢复任务状态")).toBeInTheDocument();
    expect(screen.getByText(/Glob/)).toBeInTheDocument();
    expect(document.querySelector(".ProseMirror")).toBeNull();

    await user.click(screen.getByRole("button", { name: "停止任务" }));

    await waitFor(() =>
      expect(tauriMocks.stopStreaming).toHaveBeenCalledWith("conv-1"),
    );
    await waitFor(() =>
      expect(tauriMocks.clearActiveTurnStage).toHaveBeenCalledWith("conv-1"),
    );
    expect(document.querySelector(".ProseMirror")).toBeTruthy();
  });

  it("intercepts the composer from persisted waitingInteraction stage when no pending interaction survived reload", async () => {
    const user = userEvent.setup();
    useStreamingStore.setState({
      pendingAsks: new Map(),
      streamStates: {
        "conv-1": {
          isStreaming: false,
          streamingContent: "",
          toolExecutions: [],
          turnStage: {
            kind: "waitingInteraction",
            interactionKind: "askUserQuestion",
            interactionId: "interaction-stage",
          },
          stageStartedAt: Date.now(),
          lastHeartbeatAt: Date.now(),
          turnStartedAt: Date.now(),
        },
      },
    });

    render(<ChatBottomArea />);

    expect(await screen.findByText("需要恢复任务状态")).toBeInTheDocument();
    expect(screen.getByText(/askUserQuestion/)).toBeInTheDocument();
    expect(document.querySelector(".ProseMirror")).toBeNull();

    await user.click(screen.getByRole("button", { name: "停止任务" }));

    await waitFor(() =>
      expect(tauriMocks.stopStreaming).toHaveBeenCalledWith("conv-1"),
    );
    await waitFor(() =>
      expect(tauriMocks.clearActiveTurnStage).toHaveBeenCalledWith("conv-1"),
    );
    expect(document.querySelector(".ProseMirror")).toBeTruthy();
  });

  it("restores the composer when pending action is resolved while viewing another conversation", async () => {
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    const { rerender } = render(<ChatBottomArea />);
    expect(
      await screen.findByText(/需要你允许我用提升权限只读检查/),
    ).toBeInTheDocument();

    act(() => {
      useChatStore.setState({ activeConversationId: "conv-2" });
    });
    rerender(<ChatBottomArea />);
    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );

    act(() => {
      useStreamingStore.getState().removePendingAsk("tool-1");
      useChatStore.setState({ activeConversationId: "conv-1" });
    });
    rerender(<ChatBottomArea />);

    await waitFor(() =>
      expect(document.querySelector(".ProseMirror")).toBeTruthy(),
    );
    expect(
      screen.queryByText(/需要你允许我用提升权限只读检查/),
    ).not.toBeInTheDocument();
  });

  it("replaces the composer with AskUserQuestion surface for the active conversation", async () => {
    useInteractionStore.getState().addInteraction(pendingInteraction());

    render(<ChatBottomArea />);

    expect(await screen.findByText("Need input?")).toBeInTheDocument();
    expect(document.querySelector(".ProseMirror")).toBeNull();
  });

  it("restores AskUserQuestion surface from backend snapshot when the live interaction event was missed", async () => {
    tauriMocks.pendingInteractionSnapshotForSession.mockResolvedValueOnce([
      pendingInteraction({
        interactionId: "ask-snapshot",
        payload: {
          questions: [
            {
              header: "Direction",
              question: "Which direction?",
              options: [{ label: "A", description: "Use A" }],
            },
          ],
        },
      }),
    ]);

    render(<ChatBottomArea />);

    expect(await screen.findByText("Which direction?")).toBeInTheDocument();
    expect(document.querySelector(".ProseMirror")).toBeNull();
    expect(
      useInteractionStore.getState().pendingInteractions[0]?.interactionId,
    ).toBe("ask-snapshot");
  });

  it("keeps permission pending until approve succeeds, then clears it", async () => {
    const user = userEvent.setup();
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: "提交" }));

    await waitFor(() =>
      expect(tauriMocks.approvePermissionRequest).toHaveBeenCalledWith(
        "tool-1",
        null,
        false,
        "session",
        "用户允许了这个权限申请（仅本次）。",
      ),
    );
    expect(useStreamingStore.getState().pendingAsks.has("tool-1")).toBe(false);
  });

  it("approves and clears grouped permission asks together", async () => {
    const user = userEvent.setup();
    useStreamingStore.getState().addPendingAsk(
      pendingAsk({
        toolCallId: "tool-1",
        message: "该路径未授权，需要用户确认：路径=/private/tmp/aijia/one.txt",
      }),
    );
    useStreamingStore.getState().addPendingAsk(
      pendingAsk({
        toolCallId: "tool-2",
        message: "该路径未授权，需要用户确认：路径=/private/tmp/aijia/two.txt",
      }),
    );

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: "提交" }));

    await waitFor(() => {
      expect(tauriMocks.approvePermissionRequest).toHaveBeenCalledTimes(2);
      expect(tauriMocks.approvePermissionRequest).toHaveBeenNthCalledWith(
        1,
        "tool-1",
        null,
        false,
        "session",
        "用户允许了这个权限申请（仅本次）。",
      );
      expect(tauriMocks.approvePermissionRequest).toHaveBeenNthCalledWith(
        2,
        "tool-2",
        null,
        false,
        "session",
        "用户允许了这个权限申请（仅本次）。",
      );
    });
    expect(useStreamingStore.getState().pendingAsks.has("tool-1")).toBe(false);
    expect(useStreamingStore.getState().pendingAsks.has("tool-2")).toBe(false);
  });

  it("keeps permission pending when approve fails", async () => {
    const user = userEvent.setup();
    tauriMocks.approvePermissionRequest.mockRejectedValueOnce(
      new Error("boom"),
    );
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: "提交" }));

    await waitFor(() =>
      expect(tauriMocks.approvePermissionRequest).toHaveBeenCalled(),
    );
    expect(useStreamingStore.getState().pendingAsks.has("tool-1")).toBe(true);
  });

  it("clears permission pending after reject succeeds", async () => {
    const user = userEvent.setup();
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: "拒绝" }));

    await waitFor(() =>
      expect(tauriMocks.denyPermissionRequest).toHaveBeenCalledWith(
        "tool-1",
        "用户拒绝了这个权限申请",
        false,
        "session",
      ),
    );
    expect(tauriMocks.cancelPermissionRequest).not.toHaveBeenCalled();
    expect(useStreamingStore.getState().pendingAsks.has("tool-1")).toBe(false);
  });

  it("localizes permission reject feedback sent back to the runtime", async () => {
    const user = userEvent.setup();
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    render(<ChatBottomArea />);

    i18nMock.language = "en-US";
    await user.click(await screen.findByRole("button", { name: "拒绝" }));

    await waitFor(() =>
      expect(tauriMocks.denyPermissionRequest).toHaveBeenCalledWith(
        "tool-1",
        "The user denied this permission request.",
        false,
        "session",
      ),
    );
  });

  it("keeps permission pending when reject fails", async () => {
    const user = userEvent.setup();
    tauriMocks.denyPermissionRequest.mockRejectedValueOnce(new Error("boom"));
    useStreamingStore.getState().addPendingAsk(pendingAsk());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: "拒绝" }));

    await waitFor(() =>
      expect(tauriMocks.denyPermissionRequest).toHaveBeenCalled(),
    );
    expect(useStreamingStore.getState().pendingAsks.has("tool-1")).toBe(true);
  });

  it("submits AskUserQuestion and clears it after success", async () => {
    const user = userEvent.setup();
    useInteractionStore.getState().addInteraction(pendingInteraction());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("radio", { name: "Yes" }));
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(tauriMocks.submitUserInteraction).toHaveBeenCalledWith("ask-1", {
        answers: { "Need input?": "Yes" },
        annotations: {
          userChoiceSummary: "用户回答了补充问题：Need input? = Yes",
        },
      }),
    );
    expect(useInteractionStore.getState().pendingInteractions).toEqual([]);
  });

  it("keeps AskUserQuestion pending when submit fails", async () => {
    const user = userEvent.setup();
    tauriMocks.submitUserInteraction.mockRejectedValueOnce(new Error("boom"));
    useInteractionStore.getState().addInteraction(pendingInteraction());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("radio", { name: "Yes" }));
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(tauriMocks.submitUserInteraction).toHaveBeenCalled(),
    );
    expect(useInteractionStore.getState().pendingInteractions).toHaveLength(1);
  });

  it("clears AskUserQuestion pending after cancel succeeds", async () => {
    const user = userEvent.setup();
    useInteractionStore.getState().addInteraction(pendingInteraction());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: /忽略/ }));

    await waitFor(() =>
      expect(tauriMocks.cancelUserInteraction).toHaveBeenCalledWith(
        "ask-1",
        "用户忽略了这个补充问题。请基于已有信息继续；不要输出空标题、空书名号或空占位符，缺少关键名称时请使用通用名称。",
      ),
    );
    expect(useInteractionStore.getState().pendingInteractions).toEqual([]);
  });

  it("keeps AskUserQuestion pending when cancel fails", async () => {
    const user = userEvent.setup();
    tauriMocks.cancelUserInteraction.mockRejectedValueOnce(new Error("boom"));
    useInteractionStore.getState().addInteraction(pendingInteraction());

    render(<ChatBottomArea />);

    await user.click(await screen.findByRole("button", { name: /忽略/ }));

    await waitFor(() =>
      expect(tauriMocks.cancelUserInteraction).toHaveBeenCalled(),
    );
    expect(useInteractionStore.getState().pendingInteractions).toHaveLength(1);
  });
});
