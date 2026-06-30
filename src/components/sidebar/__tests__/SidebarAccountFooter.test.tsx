import "@testing-library/jest-dom";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAuthStore } from "@/stores/authStore";
import { DEFAULTS, useBrandingStore } from "@/stores/brandingStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { setEnvironmentCache } from "@/lib/environment";
import { SidebarAccountFooter } from "../SidebarAccountFooter";

const tauriMock = vi.hoisted(() => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  setImChannelKeepAwake: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  getSettings: tauriMock.getSettings,
  updateSettings: tauriMock.updateSettings,
  setImChannelKeepAwake: tauriMock.setImChannelKeepAwake,
}));

describe("SidebarAccountFooter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setEnvironmentCache({
      tenant: "https://ai-tenant.renlijia.com",
      ops: "https://ai-ops.renlijia.com",
    });
    useAuthStore.setState({
      isLoggedIn: true,
      user: { id: 26, name: "oay xg", username: "oay@example.com" },
      tenant: { id: 15, name: "Pro", balance: "0" },
    });
    useBrandingStore.setState({
      productName: "XiaoXin",
      logoUrl: "/app-icon.png",
    });
    useSettingsStore.setState({
      appLanguage: "zh-CN",
      fontScale: "medium",
      chatWidthMode: "full",
      profileAvatarMode: "initial",
      profileAvatarEmoji: "",
      profileAvatarImagePath: "",
      imChannelKeepAwakeEnabled: false,
    });
    tauriMock.getSettings.mockResolvedValue(useSettingsStore.getState());
    tauriMock.updateSettings.mockResolvedValue(undefined);
    tauriMock.setImChannelKeepAwake.mockResolvedValue(undefined);
  });

  it("opens only the root account menu before hovering preferences", async () => {
    const user = userEvent.setup();
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    expect(screen.getByText("oay xg")).toBeInTheDocument();
    expect(screen.getByText("XiaoXin")).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    const accountTrigger = screen.getByRole("button", { name: /账户与设置/ });
    expect(accountTrigger.tagName).toBe("DIV");
    expect(accountTrigger.parentElement).toHaveClass("px-2", "py-2");
    expect(accountTrigger.parentElement).not.toHaveClass("pt-1");
    expect(accountTrigger.parentElement).not.toHaveClass("pb-2");
    expect(accountTrigger).not.toHaveClass("border", "bg-sidebar-accent/45");
    expect(accountTrigger.className).not.toContain("shadow-");
    expect(accountTrigger).not.toHaveClass("px-2", "py-2");
    expect(accountTrigger).not.toHaveClass("pt-2");
    expect(accountTrigger).not.toHaveClass("min-h-[58px]");
    expect(accountTrigger).toHaveClass("h-12");
    expect(screen.getByText("oay xg").parentElement).not.toHaveClass("gap-0.5");
    expect(accountTrigger).toHaveClass("cursor-pointer");
    expect(accountTrigger).toHaveStyle({ cursor: "pointer" });
    expect(screen.getByTestId("sidebar-account-avatar").querySelector("img")).toHaveAttribute(
      "src",
      "/app-icon.png",
    );

    await user.click(accountTrigger);

    const rootMenu = screen.getByRole("menu", { name: "账户与设置" });
    expect(rootMenu).toBeInTheDocument();
    expect(rootMenu.className).toContain("[--sidebar-account-menu-width:200px]");
    expect(rootMenu.className).toContain("w-[var(--sidebar-account-menu-width)]");
    expect(screen.getByRole("button", { name: "设置" })).toHaveClass("h-8");
    expect(screen.getByRole("button", { name: "偏好设置" })).toHaveClass("h-8");
    expect(screen.queryByRole("menu", { name: "偏好设置" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menu", { name: "字号大小" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "升级订阅" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "帮助文档" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "更新日志" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关于我们" })).toHaveClass("h-8");

    fireEvent.pointerEnter(screen.getByRole("button", { name: "偏好设置" }));

    const preferencesMenu = screen.getByRole("menu", { name: "偏好设置" });
    expect(preferencesMenu).toBeInTheDocument();
    expect(preferencesMenu).toHaveClass("absolute", "bottom-0");
    expect(screen.queryByRole("button", { name: "主题" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "字体设置" })).not.toBeInTheDocument();

    fireEvent.pointerEnter(screen.getByRole("button", { name: "关于我们" }));

    expect(screen.queryByRole("menu", { name: "偏好设置" })).not.toBeInTheDocument();

    fireEvent.pointerEnter(screen.getByRole("button", { name: "偏好设置" }));

    fireEvent.pointerEnter(screen.getByRole("button", { name: "字号大小" }));

    const fontSizeMenu = screen.getByRole("menu", { name: "字号大小" });
    expect(fontSizeMenu).toBeInTheDocument();
    expect(fontSizeMenu).toHaveClass("absolute", "bottom-0");
    expect(screen.getByRole("menuitemradio", { name: "中" })).toHaveClass("h-8");
    expect(screen.getByRole("menuitemradio", { name: "中" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("opens the account menu from the whole account footer", async () => {
    const user = userEvent.setup();
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    await user.click(screen.getByText("oay xg"));

    expect(screen.getByRole("menu", { name: "账户与设置" })).toBeInTheDocument();
  });

  it("uses the brand logo instead of the user's profile avatar", () => {
    useSettingsStore.setState({
      profileAvatarMode: "image",
      profileAvatarImagePath: "/Users/oayzz/.renlijia/users/t_15__u_152/profile/avatars/avatar.jpeg",
    });

    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    const avatarImage = screen.getByTestId("sidebar-account-avatar").querySelector("img");
    expect(avatarImage).toHaveAttribute("src", "/app-icon.png");
    expect(avatarImage?.getAttribute("src")).not.toContain("avatar.jpeg");
    expect(avatarImage).not.toHaveAttribute("alt", "当前头像");
  });

  it("falls back to the built-in brand logo when tenant branding has no logo", () => {
    useBrandingStore.setState({ logoUrl: "" });

    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    expect(screen.getByTestId("sidebar-account-avatar").querySelector("img")).toHaveAttribute(
      "src",
      DEFAULTS.logoUrl,
    );
  });

  it("persists font size settings from the cascaded preferences panel", async () => {
    const user = userEvent.setup();
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));
    fireEvent.pointerEnter(screen.getByRole("button", { name: "偏好设置" }));
    fireEvent.pointerEnter(screen.getByRole("button", { name: "字号大小" }));
    await user.click(screen.getByRole("menuitemradio", { name: "大" }));

    expect(useSettingsStore.getState().fontScale).toBe("large");
    await waitFor(() => {
      expect(tauriMock.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ fontScale: "large" }),
      );
    });
  });

  it("persists runtime keep-awake from the preferences panel", async () => {
    const user = userEvent.setup();
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));
    fireEvent.pointerEnter(screen.getByRole("button", { name: "偏好设置" }));
    fireEvent.pointerEnter(screen.getByRole("button", { name: "防休眠" }));
    expect(screen.getByText("XiaoXin 运行聊天时保持电脑唤醒")).toBeInTheDocument();
    await user.click(screen.getByRole("menuitemradio", { name: "开" }));

    expect(useSettingsStore.getState().imChannelKeepAwakeEnabled).toBe(true);
    await waitFor(() => {
      expect(tauriMock.setImChannelKeepAwake).toHaveBeenCalledWith(true);
      expect(tauriMock.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ imChannelKeepAwakeEnabled: true }),
      );
    });
  });

  it("opens full settings directly at the selected page", async () => {
    const user = userEvent.setup();
    const onOpenSettings = vi.fn();
    render(<SidebarAccountFooter onOpenSettings={onOpenSettings} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));
    await user.click(screen.getByRole("button", { name: "设置" }));

    expect(onOpenSettings).toHaveBeenCalledWith("account");
  });

  it("opens the about settings page from the account menu", async () => {
    const user = userEvent.setup();
    const onOpenSettings = vi.fn();
    render(<SidebarAccountFooter onOpenSettings={onOpenSettings} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));
    await user.click(screen.getByRole("button", { name: "关于我们" }));

    expect(onOpenSettings).toHaveBeenCalledWith("about");
    expect(screen.queryByRole("menu", { name: "账户与设置" })).not.toBeInTheDocument();
  });

  it("does not show the admin portal entry for members", async () => {
    const user = userEvent.setup();
    useAuthStore.setState({
      user: { id: 26, name: "oay xg", username: "oay@example.com", role: "member" },
    });
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));

    expect(screen.queryByRole("button", { name: "管理后台" })).not.toBeInTheDocument();
  });

  it("opens the production admin portal iframe for admins", async () => {
    const user = userEvent.setup();
    useAuthStore.setState({
      user: { id: 26, name: "oay xg", username: "oay@example.com", role: "admin" },
    });
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));
    await user.click(screen.getByRole("button", { name: "管理后台" }));

    const dialog = screen.getByRole("dialog", { name: "管理后台" });
    expect(dialog).toHaveClass("h-[90vh]", "w-[90vw]", "max-w-none");
    const heading = screen.getByRole("heading", { name: "管理后台" });
    expect(heading.parentElement).toHaveClass("h-12", "items-center", "justify-between");
    const closeButton = screen.getByLabelText(/^(Close|关闭)$/);
    expect(closeButton).not.toHaveClass("absolute", "right-4", "top-4");
    expect(closeButton).toHaveClass("h-8", "w-8");
    expect(closeButton.className).not.toContain("focus:ring");
    expect(closeButton).toHaveClass("focus-visible:ring-2", "focus-visible:ring-ring");
    expect(screen.queryByRole("menu", { name: "账户与设置" })).not.toBeInTheDocument();
    expect(screen.getByTitle("管理后台")).toHaveAttribute(
      "src",
      "https://ai-tenant.renlijia.com/members",
    );
  });

  it("opens the test admin portal iframe when the app targets test tenant", async () => {
    const user = userEvent.setup();
    setEnvironmentCache({
      tenant: "https://test-ai-tenant.renlijia.com",
      ops: "https://test-ai-ops.renlijia.com",
    });
    useAuthStore.setState({
      user: { id: 26, name: "oay xg", username: "oay@example.com", role: "admin" },
    });
    render(<SidebarAccountFooter onOpenSettings={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /账户与设置/ }));
    await user.click(screen.getByRole("button", { name: "管理后台" }));

    expect(screen.getByTitle("管理后台")).toHaveAttribute(
      "src",
      "https://test-ai-tenant.renlijia.com/members",
    );
  });
});
