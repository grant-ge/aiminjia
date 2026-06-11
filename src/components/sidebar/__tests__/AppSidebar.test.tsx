import "@testing-library/jest-dom";
import * as React from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const uiState = vi.hoisted(() => ({
  route: { kind: "home" } as {
    kind: string;
    conversationId?: string;
    sessionId?: string;
    skillId?: string;
  },
  setRoute: vi.fn(
    (next: {
      kind: string;
      conversationId?: string;
      sessionId?: string;
      skillId?: string;
    }) => {
      uiState.route = next;
    },
  ),
  sidebarTabOverride: null as string | null,
  listeners: new Set<() => void>(),
  setSidebarTab: vi.fn((tab: string) => {
    uiState.sidebarTabOverride = tab;
    if (typeof localStorage !== "undefined")
      localStorage.setItem("aijia-sidebar-tab", tab);
    uiState.listeners.forEach((l) => l());
  }),
}));

const chatState = vi.hoisted(() => ({
  activeConversationId: null as string | null,
  conversations: [] as Array<{
    id: string;
    title: string;
    workspaceName?: string | null;
    kind?: string;
  }>,
  busyConversations: new Set<string>(),
  streamStates: {} as Record<
    string,
    { isStreaming?: boolean; turnStage?: unknown }
  >,
  pendingAsks: new Map<
    string,
    {
      conversationId: string;
      runId: string;
      toolCallId: string;
      toolName: string;
      message: string;
      suggestions: string[] | null;
      mode: string;
      rememberOptions: null;
      defaultDestination: null;
    }
  >(),
}));

const channelState = vi.hoisted(() => ({
  conversations: [
    {
      sessionId: "dt-session-1",
      platform: "dingtalk",
      conversationType: "private" as const,
      displayName: "姚斌权",
      unreadCount: 0,
    },
  ],
}));

vi.mock("@/hooks/useChat", () => ({
  useChat: () => ({
    conversations: chatState.conversations,
    activeConversationId: chatState.activeConversationId,
    switchConversation: vi.fn(),
    createNewConversation: vi.fn(),
    renameConversation: vi.fn(),
    archiveConversation: vi.fn(),
    setConversationPinned: vi.fn(),
  }),
}));

vi.mock("@/stores/uiStore", () => {
  // sidebarTab derives from localStorage (mirrors the real loadPersistedSidebarTab)
  // until a setSidebarTab override is applied; setSidebarTab notifies subscribed
  // hooks so tab switches re-render like the real zustand store.
  const loadTab = () => {
    const raw =
      typeof localStorage !== "undefined"
        ? localStorage.getItem("aijia-sidebar-tab")
        : null;
    return raw === "channel" || raw === "expert-team" || raw === "employee"
      ? raw
      : "project";
  };
  const snapshot = () => ({
    route: uiState.route,
    setRoute: uiState.setRoute,
    openSettings: vi.fn(),
    sidebarTab: uiState.sidebarTabOverride ?? loadTab(),
    setSidebarTab: uiState.setSidebarTab,
    consumePendingSkill: () => null,
  });
  const useUiStore = Object.assign(
    (sel: (s: unknown) => unknown) => {
      const [, force] = React.useReducer((c: number) => c + 1, 0);
      React.useEffect(() => {
        uiState.listeners.add(force);
        return () => {
          uiState.listeners.delete(force);
        };
      }, []);
      return sel(snapshot());
    },
    {
      getState: () => snapshot(),
      subscribe: () => () => {},
      setState: () => {},
    },
  );
  return {
    useUiStore,
    useActiveConversationId: () =>
      uiState.route.kind === "chat"
        ? (uiState.route.conversationId ?? null)
        : null,
    useActiveChannelSessionId: () =>
      uiState.route.kind === "channel"
        ? (uiState.route.sessionId ?? null)
        : null,
  };
});

vi.mock("@/stores/authStore", () => ({
  useAuthStore: (sel: (s: unknown) => unknown) =>
    sel({ user: null, tenant: null }),
}));

vi.mock("@/stores/channelStore", () => ({
  useChannelStore: (sel: (s: unknown) => unknown) =>
    sel({
      platforms: {
        dingtalk: {
          platform: "dingtalk",
          capability: "available",
          configured: true,
          enabled: true,
          connection: "connected",
          config: null,
          lastConnectedAt: null,
          lastError: null,
        },
      },
      conversations: channelState.conversations,
    }),
}));

vi.mock("@/stores/brandingStore", () => ({
  useBrandingStore: (sel: (s: unknown) => unknown) =>
    sel({ productName: "仁励家网络科技(杭州)", logoUrl: "/app-icon.png" }),
}));

// hasExpertTeam() reads useChatStore.getState().conversations[].kind, so expose
// the same conversation list the useChat mock serves.
vi.mock("@/stores/chatStore", () => {
  const snap = () => ({
    conversations: chatState.conversations,
    busyConversations: chatState.busyConversations,
    streamStates: chatState.streamStates,
    pendingAsks: chatState.pendingAsks,
  });
  const useChatStore = Object.assign(
    (sel: (s: unknown) => unknown) => sel(snap()),
    {
      getState: () => snap(),
      setState: () => {},
      subscribe: () => () => {},
    },
  );
  return { useChatStore };
});

import {
  clearExpertTeam,
  setExpertTeam,
} from "@/features/expert-teams/expertTeamRegistry";
import {
  DEV_SETTINGS_STORAGE_KEY,
  useDevSettingsStore,
} from "@/stores/devSettingsStore";
import { useInteractionStore } from "@/stores/interactionStore";
import { AppSidebar } from "../AppSidebar";

describe("AppSidebar", () => {
  const originalUserAgent = navigator.userAgent;

  beforeEach(async () => {
    uiState.route = { kind: "home" };
    uiState.setRoute.mockClear();
    uiState.sidebarTabOverride = null;
    if (typeof localStorage !== "undefined")
      localStorage.removeItem("aijia-sidebar-tab");
    if (typeof localStorage !== "undefined")
      localStorage.removeItem(DEV_SETTINGS_STORAGE_KEY);
    useDevSettingsStore.setState({
      showToolErrorIcon: false,
      showRawSkillContent: false,
    });
    chatState.activeConversationId = null;
    chatState.conversations = [];
    chatState.busyConversations = new Set();
    chatState.streamStates = {};
    chatState.pendingAsks = new Map();
    channelState.conversations = [
      {
        sessionId: "dt-session-1",
        platform: "dingtalk",
        conversationType: "private",
        displayName: "姚斌权",
        unreadCount: 0,
      },
    ];
    useInteractionStore.getState().reset();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn(),
      },
    });
    await clearExpertTeam("normal-conv");
    await clearExpertTeam("expert-conv");
  });

  afterEach(() => {
    Object.defineProperty(navigator, "userAgent", {
      value: originalUserAgent,
      configurable: true,
    });
  });

  it("has sidebar background and 256 px width", () => {
    const { container } = render(<AppSidebar />);
    const aside = container.querySelector("aside");
    expect(aside?.className).toMatch(/w-\[256px\]/);
    expect(aside?.className).toMatch(/bg-sidebar/);
    expect(aside?.className).not.toMatch(/(^|\s)pt-3(\s|$)/);
  });

  it("renders TenantHeader name", () => {
    render(<AppSidebar />);
    expect(screen.getByText("仁励家网络科技(杭州)")).toBeInTheDocument();
  });

  it("does not render TenantHeader in the sidebar on Windows", () => {
    Object.defineProperty(navigator, "userAgent", {
      value: "Mozilla/5.0 (Windows NT 10.0)",
      configurable: true,
    });

    render(<AppSidebar />);

    expect(screen.queryByText("仁励家网络科技(杭州)")).not.toBeInTheDocument();
    expect(screen.queryByTestId("tenant-logo")).not.toBeInTheDocument();
  });

  it("opens the dev panel after seven tenant header clicks and persists the tool error diagnostics switch", async () => {
    const user = userEvent.setup();

    render(<AppSidebar />);

    const tenantHeader = screen.getByRole("button", { name: /仁励家网络科技/ });
    for (let i = 0; i < 7; i += 1) {
      await user.click(tenantHeader);
    }

    expect(
      screen.getByRole("dialog", { name: "控制面板" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText("隐藏功能和高级操作入口，不会出现在常规设置中。"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/开发调试|仅影响当前设备/),
    ).not.toBeInTheDocument();
    const displayGroup = screen.getByRole("region", { name: "显示" });
    expect(
      within(displayGroup).getByRole("heading", { name: "显示" }),
    ).toBeInTheDocument();
    const switchControl = screen.getByRole("switch", {
      name: "显示工具失败图标",
    });
    expect(
      within(displayGroup).getByRole("switch", { name: "显示工具失败图标" }),
    ).toBe(switchControl);
    expect(switchControl).not.toBeChecked();

    await user.click(switchControl);

    expect(switchControl).toBeChecked();
    expect(
      JSON.parse(localStorage.getItem(DEV_SETTINGS_STORAGE_KEY) ?? "{}"),
    ).toMatchObject({
      showToolErrorIcon: true,
    });

    const rawSkillSwitch = screen.getByRole("switch", {
      name: "显示技能原始内容",
    });
    expect(rawSkillSwitch).not.toBeChecked();

    await user.click(rawSkillSwitch);

    expect(rawSkillSwitch).toBeChecked();
    expect(
      JSON.parse(localStorage.getItem(DEV_SETTINGS_STORAGE_KEY) ?? "{}"),
    ).toMatchObject({
      showToolErrorIcon: true,
      showRawSkillContent: true,
    });
  });

  it("keeps the dev panel open when clicking the modal overlay", async () => {
    const user = userEvent.setup();

    render(<AppSidebar />);

    const tenantHeader = screen.getByRole("button", { name: /仁励家网络科技/ });
    for (let i = 0; i < 7; i += 1) {
      await user.click(tenantHeader);
    }

    expect(
      screen.getByRole("dialog", { name: "控制面板" }),
    ).toBeInTheDocument();
    await user.click(
      document.querySelector('[data-slot="dialog-overlay"]') as HTMLElement,
    );

    expect(
      screen.getByRole("dialog", { name: "控制面板" }),
    ).toBeInTheDocument();
  });

  it("renders the main nav items, the project tab, and footer 设置", () => {
    render(<AppSidebar />);
    expect(screen.getByRole("button", { name: "新任务" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "数字员工" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "专家团" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "技能中心" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "定时任务" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "IM 频道" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "项目" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "设置" })).toBeInTheDocument();
  });

  it("places IM 频道 directly after 定时任务 in the main sidebar nav", () => {
    render(<AppSidebar />);

    const buttons = screen
      .getAllByRole("button")
      .map((button) => button.textContent?.trim());
    const schedulesIndex = buttons.indexOf("定时任务");

    expect(schedulesIndex).toBeGreaterThanOrEqual(0);
    expect(buttons[schedulesIndex + 1]).toBe("IM 频道");
  });

  it("does not highlight 数字员工 while a chat route is active", () => {
    uiState.route = { kind: "chat", conversationId: "conv-1" };

    render(<AppSidebar />);

    expect(
      screen.getByRole("button", { name: "数字员工" }).className,
    ).not.toMatch(/(^|\s)bg-sidebar-accent(\s|$)/);
  });

  it("highlights 数字员工 when employees route is active", () => {
    uiState.route = { kind: "employees" };

    render(<AppSidebar />);

    expect(screen.getByRole("button", { name: "数字员工" }).className).toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    );
    expect(
      screen.getByRole("button", { name: "新任务" }).className,
    ).not.toMatch(/(^|\s)bg-sidebar-accent(\s|$)/);
  });

  it("does not highlight an active conversation while schedules route is active", () => {
    uiState.route = { kind: "schedules" };
    chatState.activeConversationId = "conv-1";
    chatState.conversations = [
      { id: "conv-1", title: "旧对话", workspaceName: "txl" },
    ];

    render(<AppSidebar />);

    expect(
      screen.getByRole("button", { name: /旧对话/ }).className,
    ).not.toMatch(/(^|\s)bg-sidebar-accent(\s|$)/);
  });

  it("shows a compact loading indicator in the conversation action slot", () => {
    chatState.conversations = [
      { id: "conv-loading", title: "运行中的对话", workspaceName: "默认项目" },
    ];
    chatState.busyConversations = new Set(["conv-loading"]);

    const { container } = render(<AppSidebar />);

    expect(
      screen.getByRole("button", { name: "运行中的对话" }),
    ).toBeInTheDocument();
    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument();
  });

  it("shows permission review chip for regular conversations waiting on permission ask", () => {
    chatState.conversations = [
      { id: "conv-pending", title: "等审批的对话", workspaceName: "默认项目" },
    ];
    chatState.pendingAsks = new Map([
      [
        "call-1",
        {
          conversationId: "conv-pending",
          runId: "run-1",
          toolCallId: "call-1",
          toolName: "Read",
          message: "需要授权",
          suggestions: null,
          mode: "default",
          rememberOptions: null,
          defaultDestination: null,
        },
      ],
    ]);

    render(<AppSidebar />);

    expect(screen.getByText("审核")).toBeInTheDocument();
    expect(screen.queryByText("审批")).not.toBeInTheDocument();
  });

  it("shows waiting reply chip for regular conversations waiting on ask user question", () => {
    chatState.conversations = [
      { id: "conv-question", title: "等回复的对话", workspaceName: "默认项目" },
    ];
    useInteractionStore.getState().addInteraction({
      conversationId: "conv-question",
      runId: "run-1",
      interactionId: "ask-1",
      toolCallId: "tool-1",
      toolName: "AskUserQuestion",
      kind: "askUserQuestion",
      payload: {
        questions: [{ question: "想写什么", header: "主题", options: [] }],
      },
    });

    render(<AppSidebar />);

    expect(screen.getByText("等待回复")).toBeInTheDocument();
  });

  it("regular conversation pending status takes priority over loading", () => {
    chatState.conversations = [
      {
        id: "conv-priority",
        title: "等审批且运行中",
        workspaceName: "默认项目",
      },
    ];
    chatState.busyConversations = new Set(["conv-priority"]);
    chatState.pendingAsks = new Map([
      [
        "call-1",
        {
          conversationId: "conv-priority",
          runId: "run-1",
          toolCallId: "call-1",
          toolName: "Read",
          message: "需要授权",
          suggestions: null,
          mode: "default",
          rememberOptions: null,
          defaultDestination: null,
        },
      ],
    ]);

    const { container } = render(<AppSidebar />);

    expect(screen.getByText("审核")).toBeInTheDocument();
    expect(
      container.querySelector('[data-icon="loader"]'),
    ).not.toBeInTheDocument();
  });

  it("separates expert team conversations from the project tab into an expert team tab", async () => {
    chatState.conversations = [
      { id: "normal-conv", title: "普通项目对话", workspaceName: "默认项目" },
      // expert-team membership is now derived from conversation.kind, which
      // hasExpertTeam() reads off useChatStore.
      {
        id: "expert-conv",
        title: "市场方案专家讨论",
        workspaceName: "默认项目",
        kind: "expertTeam",
      },
    ];
    await setExpertTeam("expert-conv", "marketing");

    render(<AppSidebar />);

    // "专家团" is a top-level nav item; the sidebar body tab is labelled "专家".
    expect(screen.getByRole("button", { name: "专家团" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "专家" })).toBeInTheDocument();
    expect(screen.getByText("普通项目对话")).toBeInTheDocument();
    expect(screen.queryByText("市场方案专家讨论")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "专家" }));

    expect(screen.getByText("市场方案专家讨论")).toBeInTheDocument();
    expect(screen.queryByText("普通项目对话")).not.toBeInTheDocument();
    expect(screen.queryByText("默认项目")).not.toBeInTheDocument();
  });

  it("switches the sidebar body between 项目 and 频道 tabs without changing route", async () => {
    render(<AppSidebar />);

    expect(
      screen.getAllByRole("button", { name: "项目" }).length,
    ).toBeGreaterThan(0);
    expect(
      screen.getAllByRole("button", { name: "频道" }).length,
    ).toBeGreaterThan(0);
    await userEvent.click(screen.getByRole("button", { name: "频道" }));
    // Tab switching is local UI state — must not affect route.
    expect(uiState.setRoute).not.toHaveBeenCalled();
    // The channel tab lists every platform section; the dingtalk private
    // conversation renders as a privacy-normalized "钉钉私聊" label (not the raw
    // push name) per the 2026-05-21 channel label redesign.
    expect(screen.getByText("钉钉")).toBeInTheDocument();
    expect(screen.getByText("钉钉私聊")).toBeInTheDocument();
  });

  it("copies only the channel conversation id from the channel row context menu", async () => {
    localStorage.setItem("aijia-sidebar-tab", "channel");
    render(<AppSidebar />);

    fireEvent.contextMenu(screen.getByRole("button", { name: "钉钉私聊" }));

    const copyItem = await screen.findByRole("menuitem", {
      name: "复制对话 ID",
    });
    expect(
      screen.queryByRole("menuitem", { name: "重命名聊天" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("menuitem", { name: "归档聊天" }),
    ).not.toBeInTheDocument();

    await userEvent.click(copyItem);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("dt-session-1");
  });

  it("shows permission review chip for channel conversations waiting on permission ask", () => {
    localStorage.setItem("aijia-sidebar-tab", "channel");
    chatState.pendingAsks = new Map([
      [
        "call-1",
        {
          conversationId: "dt-session-1",
          runId: "run-1",
          toolCallId: "call-1",
          toolName: "Read",
          message: "需要授权",
          suggestions: null,
          mode: "default",
          rememberOptions: null,
          defaultDestination: null,
        },
      ],
    ]);

    render(<AppSidebar />);

    expect(screen.getByText("审核")).toBeInTheDocument();
  });

  it("shows waiting reply chip for channel conversations waiting on ask user question", () => {
    localStorage.setItem("aijia-sidebar-tab", "channel");
    useInteractionStore.getState().addInteraction({
      conversationId: "dt-session-1",
      runId: "run-1",
      interactionId: "ask-1",
      toolCallId: "tool-1",
      toolName: "AskUserQuestion",
      kind: "askUserQuestion",
      payload: {
        questions: [{ question: "想写什么", header: "主题", options: [] }],
      },
    });

    render(<AppSidebar />);

    expect(screen.getByText("等待回复")).toBeInTheDocument();
  });

  it("shows loading indicator for busy channel conversations", () => {
    localStorage.setItem("aijia-sidebar-tab", "channel");
    chatState.busyConversations = new Set(["dt-session-1"]);

    const { container } = render(<AppSidebar />);

    expect(
      screen.getByRole("button", { name: /钉钉私聊/ }),
    ).toBeInTheDocument();
    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument();
  });

  it("channel conversation pending status takes priority over unread count", () => {
    localStorage.setItem("aijia-sidebar-tab", "channel");
    channelState.conversations = [
      {
        sessionId: "dt-session-1",
        platform: "dingtalk",
        conversationType: "private",
        displayName: "姚斌权",
        unreadCount: 7,
      },
    ];
    chatState.pendingAsks = new Map([
      [
        "call-1",
        {
          conversationId: "dt-session-1",
          runId: "run-1",
          toolCallId: "call-1",
          toolName: "Read",
          message: "需要授权",
          suggestions: null,
          mode: "default",
          rememberOptions: null,
          defaultDestination: null,
        },
      ],
    ]);

    render(<AppSidebar />);

    expect(screen.getByText("审核")).toBeInTheDocument();
    expect(screen.queryByText("7")).not.toBeInTheDocument();
  });
});

describe("AppSidebar route-derived sidebarTab", () => {
  beforeEach(() => {
    localStorage.removeItem("aijia-sidebar-tab");
    uiState.sidebarTabOverride = null;
    chatState.busyConversations = new Set();
    chatState.streamStates = {};
    chatState.pendingAsks = new Map();
    channelState.conversations = [
      {
        sessionId: "dt-session-1",
        platform: "dingtalk",
        conversationType: "private",
        displayName: "姚斌权",
        unreadCount: 0,
      },
    ];
    useInteractionStore.getState().reset();
  });

  it("shows channel list after fresh mount when channel tab persisted", async () => {
    localStorage.setItem("aijia-sidebar-tab", "channel");
    uiState.route = { kind: "channel", sessionId: "dt-session-1" };
    chatState.activeConversationId = null;
    chatState.conversations = [];
    await clearExpertTeam("normal-conv");
    await clearExpertTeam("expert-conv");
    render(<AppSidebar />);
    // dingtalk private conversation renders as the normalized "钉钉私聊" label
    const platformHeading = screen.getByText("钉钉");
    const activeChannelRow = screen.getByRole("button", { name: "钉钉私聊" });
    expect(activeChannelRow).toBeInTheDocument();
    expect(platformHeading.className).toMatch(/(^|\s)font-medium(\s|$)/);
    expect(platformHeading.className).not.toMatch(/(^|\s)font-semibold(\s|$)/);
    expect(activeChannelRow.className).toMatch(/(^|\s)font-medium(\s|$)/);
    expect(activeChannelRow.className).not.toMatch(/(^|\s)font-semibold(\s|$)/);
  });

  it("clicking IM 频道 nav while a session is selected resets route to channel overview", async () => {
    uiState.route = { kind: "channel", sessionId: "dt-session-1" };
    chatState.activeConversationId = null;
    chatState.conversations = [];
    await clearExpertTeam("normal-conv");
    await clearExpertTeam("expert-conv");
    render(<AppSidebar />);
    await userEvent.click(screen.getByRole("button", { name: "IM 频道" }));
    expect(uiState.setRoute).toHaveBeenCalledWith({ kind: "channel" });
  });

  it("highlights skill-center nav when route is skill-detail", async () => {
    uiState.route = { kind: "skill-detail", skillId: "sk-1" };
    chatState.activeConversationId = null;
    chatState.conversations = [];
    await clearExpertTeam("normal-conv");
    await clearExpertTeam("expert-conv");
    render(<AppSidebar />);
    expect(screen.getByRole("button", { name: "技能中心" }).className).toMatch(
      /(^|\s)bg-sidebar-accent(\s|$)/,
    );
  });

  it("does NOT highlight IM 频道 nav when a specific session is selected (leaf-only)", async () => {
    uiState.route = { kind: "channel", sessionId: "dt-session-1" };
    chatState.activeConversationId = null;
    chatState.conversations = [];
    await clearExpertTeam("normal-conv");
    await clearExpertTeam("expert-conv");
    render(<AppSidebar />);
    expect(
      screen.getByRole("button", { name: "IM 频道" }).className,
    ).not.toMatch(/(^|\s)bg-sidebar-accent(\s|$)/);
  });
});
