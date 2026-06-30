import "@testing-library/jest-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { PendingAsk } from "@/stores/streamingStore";
import type { InteractionRequiredPayload } from "@/lib/tauri";
import i18n from "@/i18n";
import { PendingActionSurface } from "../PendingActionSurface";

function permissionAsk(overrides: Partial<PendingAsk> = {}): PendingAsk {
  return {
    conversationId: "conv-1",
    runId: "run-1",
    toolCallId: "tool-1",
    toolName: "Read",
    message: "Read /tmp/a.txt?",
    suggestions: ["Only allow if the path is expected."],
    mode: "default",
    rememberOptions: ["session", "workspace", "user"],
    defaultDestination: "session",
    ...overrides,
  };
}

function userQuestion(
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
          header: "Branch",
          question: "Which branch?",
          options: [
            { label: "main", description: "Use main branch" },
            { label: "dev", description: "Use dev branch" },
          ],
        },
      ],
    },
    ...overrides,
  };
}

function renderSurface(
  action: React.ComponentProps<typeof PendingActionSurface>["action"],
  handlers: Partial<
    Omit<React.ComponentProps<typeof PendingActionSurface>, "action">
  > = {},
) {
  return render(
    <PendingActionSurface
      action={action}
      onAllowPermission={handlers.onAllowPermission ?? vi.fn()}
      onDenyPermission={handlers.onDenyPermission ?? vi.fn()}
      onCancelPermission={handlers.onCancelPermission ?? vi.fn()}
      onSubmitInteraction={handlers.onSubmitInteraction ?? vi.fn()}
      onCancelInteraction={handlers.onCancelInteraction ?? vi.fn()}
      onClearStalePermission={handlers.onClearStalePermission ?? vi.fn()}
      onClearStaleInteraction={handlers.onClearStaleInteraction ?? vi.fn()}
    />,
  );
}

describe("PendingActionSurface", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("zh-CN");
  });

  it("allows a permission request with selected remember destination", async () => {
    const user = userEvent.setup();
    const onAllowPermission = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      { kind: "permission", ask: permissionAsk() },
      { onAllowPermission },
    );

    expect(screen.queryByText("需要权限确认")).not.toBeInTheDocument();
    expect(
      screen.getByText(/需要你允许我用提升权限只读检查/),
    ).toBeInTheDocument();
    expect(screen.getAllByText("ls -la /tmp/a.txt").length).toBeGreaterThan(0);
    expect(
      screen.queryByText("Only allow if the path is expected."),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /^是$/ })).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /后续同类操作不再询问/ }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "否，请告知如何调整" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "提交" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "拒绝" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "拒绝" }).parentElement).toHaveClass(
      "ml-auto",
    );

    await user.click(
      screen.getByRole("radio", { name: /后续同类操作不再询问/ }),
    );
    await user.click(screen.getByRole("button", { name: "提交" }));

    await waitFor(() =>
      expect(onAllowPermission).toHaveBeenCalledWith("tool-1", {
        remember: true,
        destination: "user",
      }),
    );
  });

  it("wraps long permission paths and command previews", () => {
    const longPath =
      "/Users/oayzz/.renlijia/users/t_15__u_26/project_memories/renlijia-440238e8bacfe7d1/entries/20260511-076a895de6b0380b.md";
    const commandPreview = `ls -la ${longPath}`;

    renderSurface({
      kind: "permission",
      ask: permissionAsk({
        message: `该路径未授权，需要用户确认：路径=${longPath}`,
      }),
    });

    expect(screen.getByText(/需要你允许我用提升权限只读检查/)).toHaveClass(
      "break-all",
    );
    expect(screen.getAllByText(commandPreview)[0]).toHaveClass("break-all");

    const rememberOption = screen
      .getByRole("radio", {
        name: "是，且对于后续同类操作不再询问",
      })
      .closest("label");
    expect(rememberOption).toHaveClass("items-start");
    expect(screen.getAllByText(commandPreview)).toHaveLength(1);
  });

  it("localizes permission copy and avoids OS-specific privacy wording", async () => {
    await i18n.changeLanguage("en-US");

    renderSurface({
      kind: "permission",
      ask: permissionAsk({
        message: "Permission required: path=/Users/oayzz/Library/Messages",
      }),
    });

    expect(
      screen.getByText(/I need your permission to use elevated access/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/macOS/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/privacy-protected/i)).not.toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /^Yes$/ })).toBeChecked();
    expect(
      screen.getByRole("radio", { name: /do not ask again/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "No, explain how to adjust" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Submit" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reject" })).toBeInTheDocument();
  });

  it("uses a Windows-safe command preview for Windows paths", () => {
    const windowsPath = "C:\\Users\\oayzz\\Documents\\secret.txt";
    const commandPreview = `dir /a ${windowsPath}`;

    renderSurface({
      kind: "permission",
      ask: permissionAsk({
        message: `Permission required: path=${windowsPath}`,
      }),
    });

    expect(screen.getAllByText(commandPreview).length).toBeGreaterThan(0);
    expect(screen.queryByText(`ls -la ${windowsPath}`)).not.toBeInTheDocument();
  });

  it("resets permission destination when a new permission action has narrower remember options", async () => {
    const user = userEvent.setup();
    const onAllowPermission = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <PendingActionSurface
        action={{ kind: "permission", ask: permissionAsk() }}
        onAllowPermission={onAllowPermission}
        onDenyPermission={vi.fn()}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={vi.fn()}
        onCancelInteraction={vi.fn()}
        onClearStalePermission={vi.fn()}
        onClearStaleInteraction={vi.fn()}
      />,
    );

    await user.click(
      screen.getByRole("radio", { name: /后续同类操作不再询问/ }),
    );

    rerender(
      <PendingActionSurface
        action={{
          kind: "permission",
          ask: permissionAsk({
            toolCallId: "tool-2",
            toolName: "Write",
            rememberOptions: ["session"],
            defaultDestination: "session",
          }),
        }}
        onAllowPermission={onAllowPermission}
        onDenyPermission={vi.fn()}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={vi.fn()}
        onCancelInteraction={vi.fn()}
        onClearStalePermission={vi.fn()}
        onClearStaleInteraction={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "提交" }));

    await waitFor(() =>
      expect(onAllowPermission).toHaveBeenLastCalledWith("tool-2", {
        remember: false,
        destination: "session",
      }),
    );
  });

  it("guards permission allow against duplicate clicks while in flight", async () => {
    const user = userEvent.setup();
    let resolveAllow: () => void = () => {};
    const onAllowPermission = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveAllow = resolve;
        }),
    );

    renderSurface(
      { kind: "permission", ask: permissionAsk() },
      { onAllowPermission },
    );

    const allowButton = screen.getByRole("button", { name: "提交" });
    await user.dblClick(allowButton);

    expect(onAllowPermission).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveAllow();
    });
  });

  it("does not render a free-form permission answer row", () => {
    renderSurface({
      kind: "permission",
      ask: permissionAsk(),
    });

    expect(
      screen.queryByRole("textbox", { name: /否，请告知/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("3.")).not.toBeInTheDocument();
  });

  it("denies a permission request from the reject button", async () => {
    const user = userEvent.setup();
    const onCancelPermission = vi.fn().mockResolvedValue(undefined);
    const onDenyPermission = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      { kind: "permission", ask: permissionAsk() },
      { onCancelPermission, onDenyPermission },
    );

    await user.click(screen.getByRole("button", { name: "拒绝" }));

    await waitFor(() =>
      expect(onDenyPermission).toHaveBeenCalledWith("tool-1", {
        remember: false,
        destination: "session",
      }),
    );
    expect(onCancelPermission).not.toHaveBeenCalled();
  });

  it("applies one permission decision to every request in a grouped permission action", async () => {
    const user = userEvent.setup();
    const onAllowPermission = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      {
        kind: "permission-group",
        asks: [
          permissionAsk({
            toolCallId: "tool-1",
            message: "该路径未授权，需要用户确认：路径=/tmp/aijia/one.txt",
          }),
          permissionAsk({
            toolCallId: "tool-2",
            message: "该路径未授权，需要用户确认：路径=/tmp/aijia/two.txt",
          }),
        ],
      },
      { onAllowPermission },
    );

    expect(screen.getByText(/2 个权限请求/)).toBeInTheDocument();
    expect(screen.getByText("/tmp/aijia/one.txt")).toBeInTheDocument();
    expect(screen.getByText("/tmp/aijia/two.txt")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "提交" }));

    await waitFor(() => {
      expect(onAllowPermission).toHaveBeenCalledTimes(2);
      expect(onAllowPermission).toHaveBeenNthCalledWith(1, "tool-1", {
        remember: false,
        destination: "session",
      });
      expect(onAllowPermission).toHaveBeenNthCalledWith(2, "tool-2", {
        remember: false,
        destination: "session",
      });
    });
  });

  it("shows the tool name for each request in a grouped permission action", () => {
    renderSurface({
      kind: "permission-group",
      asks: [
        permissionAsk({
          toolCallId: "tool-1",
          toolName: "Grep",
          message: "该路径未授权，需要用户确认：路径=/private/tmp",
        }),
        permissionAsk({
          toolCallId: "tool-2",
          toolName: "Glob",
          message: "该路径未授权，需要用户确认：路径=/private/tmp",
        }),
      ],
    });

    expect(screen.getByText("Grep")).toBeInTheDocument();
    expect(screen.getByText("Glob")).toBeInTheDocument();
    expect(screen.getByText(/2 个权限请求/)).toBeInTheDocument();
  });

  it("lays out grouped permission requests horizontally", () => {
    renderSurface({
      kind: "permission-group",
      asks: [
        permissionAsk({
          toolCallId: "tool-1",
          toolName: "Grep",
          message: "该路径未授权，需要用户确认：路径=/private/tmp",
        }),
        permissionAsk({
          toolCallId: "tool-2",
          toolName: "Glob",
          message: "该路径未授权，需要用户确认：路径=/private/tmp",
        }),
      ],
    });

    const requestList = screen.getByLabelText("权限请求列表");
    expect(requestList).toHaveClass("flex", "flex-wrap");
    expect(screen.getAllByRole("listitem")[0]).toHaveClass("inline-flex");
  });

  it("renders permission action buttons in the compact size", () => {
    renderSurface({ kind: "permission", ask: permissionAsk() });

    expect(screen.getByRole("button", { name: "拒绝" })).toHaveClass("h-6");
    expect(screen.getByRole("button", { name: "提交" })).toHaveClass("h-6");
    expect(screen.getByRole("button", { name: "提交" })).not.toHaveClass("h-9");
  });

  it("only shades the selected permission option instead of the whole option group", () => {
    renderSurface({ kind: "permission", ask: permissionAsk() });

    const selectedOption = screen
      .getByRole("radio", { name: /^是$/ })
      .closest("label");
    const optionGroup = selectedOption?.parentElement;

    expect(selectedOption).toHaveClass("bg-[rgba(var(--muted-rgb),0.60)]");
    expect(selectedOption).not.toHaveClass("bg-background");
    expect(optionGroup).not.toHaveClass("bg-[rgba(var(--muted-rgb),0.30)]");
  });

  it("submits a selected user question answer", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      { kind: "user-question", interaction: userQuestion() },
      { onSubmitInteraction },
    );

    await user.click(screen.getByRole("radio", { name: /main/ }));
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(onSubmitInteraction).toHaveBeenCalledWith("ask-1", {
        answers: { "Which branch?": "main" },
        annotations: {
          userChoiceSummary: "用户回答了补充问题：Which branch? = main",
        },
      }),
    );
  });

  it("shows AskUserQuestion as a single-question picker with pager controls", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      {
        kind: "user-question",
        interaction: userQuestion({
          payload: {
            questions: [
              {
                header: "Budget",
                question: "单人预算大概希望控制在什么范围？",
                options: [
                  { label: "3000 内 (Recommended)", description: "自然放松" },
                  { label: "3000-6000", description: "舒适一点" },
                ],
              },
              {
                header: "Style",
                question: "旅行风格更偏向什么？",
                options: [
                  { label: "自然放松", description: "少赶路" },
                  { label: "城市探索", description: "多逛逛" },
                ],
              },
            ],
          },
        }),
      },
      { onSubmitInteraction },
    );

    expect(
      screen.getByText("单人预算大概希望控制在什么范围？"),
    ).toBeInTheDocument();
    expect(screen.queryByText("旅行风格更偏向什么？")).not.toBeInTheDocument();
    expect(screen.getByText("1 of 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "继续" })).not.toBeDisabled();

    await user.click(screen.getByRole("radio", { name: /3000 内/ }));
    expect(screen.getByText("旅行风格更偏向什么？")).toBeInTheDocument();
    expect(
      screen.queryByText("单人预算大概希望控制在什么范围？"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("2 of 2")).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /自然放松/ }));
    expect(
      screen.getByRole("radio", { name: /自然放松/ }).closest("label"),
    ).toHaveClass("bg-[rgba(var(--muted-rgb),0.60)]");
    expect(
      screen.getByRole("radio", { name: /自然放松/ }).closest("label")
        ?.parentElement,
    ).not.toHaveClass("bg-[rgba(var(--muted-rgb),0.30)]");
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(onSubmitInteraction).toHaveBeenCalledWith("ask-1", {
        answers: {
          "单人预算大概希望控制在什么范围？": "3000 内 (Recommended)",
          "旅行风格更偏向什么？": "自然放松",
        },
        annotations: {
          userChoiceSummary:
            "用户回答了补充问题：单人预算大概希望控制在什么范围？ = 3000 内 (Recommended)；旅行风格更偏向什么？ = 自然放松",
        },
      }),
    );
  });

  it("submits AskUserQuestion with Enter after all answers are selected", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      {
        kind: "user-question",
        interaction: userQuestion({
          payload: {
            questions: [
              {
                header: "Target",
                question: "测试哪个目标？",
                options: [
                  { label: "婴喜爱官网", description: "测试官网" },
                  { label: "婴喜爱手机APP", description: "测试 APP" },
                ],
              },
              {
                header: "Scope",
                question: "测试范围？",
                options: [
                  { label: "功能测试", description: "只测功能" },
                  { label: "全面测试", description: "功能和 UI 都测" },
                ],
              },
            ],
          },
        }),
      },
      { onSubmitInteraction },
    );

    await user.click(screen.getByRole("radio", { name: "婴喜爱官网" }));
    await user.click(screen.getByRole("radio", { name: "全面测试" }));
    await user.keyboard("{Enter}");

    await waitFor(() =>
      expect(onSubmitInteraction).toHaveBeenCalledWith("ask-1", {
        answers: {
          "测试哪个目标？": "婴喜爱官网",
          "测试范围？": "全面测试",
        },
        annotations: {
          userChoiceSummary:
            "用户回答了补充问题：测试哪个目标？ = 婴喜爱官网；测试范围？ = 全面测试",
        },
      }),
    );
  });

  it("exposes stable e2e selectors for the active AskUserQuestion surface", () => {
    renderSurface({ kind: "user-question", interaction: userQuestion() });

    const surface = document.querySelector(
      '[data-aijia-pending-action="user-question"]',
    );
    expect(surface).toBeTruthy();
    expect(surface).toHaveAttribute(
      "data-aijia-pending-action-tool",
      "AskUser",
    );
    expect(surface).toHaveAttribute(
      "data-aijia-pending-action-interaction-id",
      "ask-1",
    );
    expect(
      surface?.querySelector("[data-aijia-pending-action-title]"),
    ).toHaveTextContent("Which branch?");

    const options = Array.from(
      surface?.querySelectorAll(
        '[data-aijia-pending-action-action="option"]',
      ) ?? [],
    );
    expect(options).toHaveLength(3);
    expect(options[0]).toHaveAttribute(
      "data-aijia-pending-action-option-label",
      "main",
    );
    expect(options[0]).toHaveAttribute(
      "data-aijia-pending-action-question-index",
      "0",
    );
    expect(options[0]).toHaveAttribute(
      "data-aijia-pending-action-option-index",
      "0",
    );
    expect(options[2]).toHaveAttribute(
      "data-aijia-pending-action-option-label",
      "__other__",
    );
  });

  it("advances to the next AskUserQuestion after selecting a single-choice option", async () => {
    const user = userEvent.setup();

    renderSurface({
      kind: "user-question",
      interaction: userQuestion({
        payload: {
          questions: [
            {
              header: "Budget",
              question: "预算是多少？",
              options: [{ label: "3000 内", description: "推荐" }],
            },
            {
              header: "Style",
              question: "旅行风格？",
              options: [{ label: "自然放松", description: "少赶路" }],
            },
          ],
        },
      }),
    });

    await user.click(screen.getByRole("radio", { name: "3000 内" }));

    expect(screen.getByText("旅行风格？")).toBeInTheDocument();
    expect(screen.getByText("2 of 2")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "上一题" }));
    expect(screen.getByRole("radio", { name: "3000 内" })).toBeChecked();
  });

  it("adds a built-in custom answer row for AskUserQuestion options", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      { kind: "user-question", interaction: userQuestion() },
      { onSubmitInteraction },
    );

    await user.click(screen.getByRole("radio", { name: "其他" }));
    const customAnswer = screen.getByRole("textbox", { name: "其他" });
    await user.type(customAnswer, "Use the release branch instead");
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(onSubmitInteraction).toHaveBeenCalledWith("ask-1", {
        answers: { "Which branch?": "Use the release branch instead" },
        annotations: {
          userChoiceSummary:
            "用户回答了补充问题：Which branch? = Use the release branch instead",
        },
      }),
    );
  });

  it("uses tight inherited typography for the custom answer textarea caret", async () => {
    const user = userEvent.setup();

    renderSurface({ kind: "user-question", interaction: userQuestion() });

    await user.click(screen.getByRole("radio", { name: "其他" }));
    const customAnswer = screen.getByRole("textbox", { name: "其他" });

    expect(customAnswer).toHaveClass("text-sm");
    expect(customAnswer).toHaveClass("leading-5");
    expect(customAnswer).toHaveClass("[font:inherit]");
    expect(customAnswer).not.toHaveClass("leading-6");
  });

  it("does not duplicate an AskUserQuestion custom row when the tool already provides Other", () => {
    renderSurface({
      kind: "user-question",
      interaction: userQuestion({
        payload: {
          questions: [
            {
              header: "Branch",
              question: "Which branch?",
              options: [
                { label: "main", description: "Use main branch" },
                { label: "Other", description: "Custom answer" },
              ],
            },
          ],
        },
      }),
    });

    expect(screen.getAllByRole("radio", { name: "其他" })).toHaveLength(1);
  });

  it("keeps business options that merely start with the custom label", () => {
    renderSurface({
      kind: "user-question",
      interaction: userQuestion({
        payload: {
          questions: [
            {
              header: "Direction",
              question: "这个方案是哪个方向？",
              options: [
                { label: "账单核对自动化", description: "A" },
                { label: "其他项目", description: "Custom answer" },
              ],
            },
          ],
        },
      }),
    });

    expect(screen.getByRole("radio", { name: "其他项目" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio", { name: "其他" })).toHaveLength(1);
  });

  it("jumps to the first unanswered AskUserQuestion page before submitting", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      {
        kind: "user-question",
        interaction: userQuestion({
          payload: {
            questions: [
              {
                header: "Budget",
                question: "预算是多少？",
                options: [{ label: "3000 内", description: "推荐" }],
              },
              {
                header: "Style",
                question: "旅行风格？",
                options: [{ label: "自然放松", description: "少赶路" }],
              },
            ],
          },
        }),
      },
      { onSubmitInteraction },
    );

    await user.click(screen.getByRole("button", { name: "下一题" }));
    await user.click(screen.getByRole("radio", { name: "自然放松" }));
    await user.click(screen.getByRole("button", { name: "继续" }));

    expect(screen.getByText("预算是多少？")).toBeInTheDocument();
    expect(screen.getByText("1 of 2")).toBeInTheDocument();
    expect(screen.getByText("请先回答这一题。")).toBeInTheDocument();
    expect(onSubmitInteraction).not.toHaveBeenCalled();
  });

  it("shows outer tabs only when multiple pending actions are available", async () => {
    const user = userEvent.setup();

    renderSurface([
      { kind: "permission", ask: permissionAsk({ toolCallId: "tool-read" }) },
      {
        kind: "user-question",
        interaction: userQuestion({ interactionId: "ask-plan" }),
      },
    ]);

    expect(screen.getByRole("tablist", { name: "待处理事项" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "权限 Read" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: "问题 1" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    expect(screen.queryByText("Which branch?")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "问题 1" }));

    expect(screen.getByRole("tab", { name: "问题 1" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Which branch?")).toBeInTheDocument();
  });

  it("shows AskUserQuestion option descriptions in tooltips next to the option text", async () => {
    const user = userEvent.setup();

    renderSurface({ kind: "user-question", interaction: userQuestion() });

    const mainOption = screen
      .getByRole("radio", { name: "main" })
      .closest("label");
    const infoButton = screen.getByRole("button", { name: "Use main branch" });

    expect(mainOption).toContainElement(infoButton);
    await user.hover(infoButton);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Use main branch",
    );
  });

  it("renders AskUserQuestion action buttons in the compact size", () => {
    renderSurface({ kind: "user-question", interaction: userQuestion() });

    expect(screen.getByRole("button", { name: "忽略" })).toHaveClass("h-6");
    expect(screen.getByRole("button", { name: "继续" })).toHaveClass("h-6");
  });

  it("does not reuse answers when a new AskUserQuestion action replaces the previous one", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <PendingActionSurface
        action={{ kind: "user-question", interaction: userQuestion() }}
        onAllowPermission={vi.fn()}
        onDenyPermission={vi.fn()}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={onSubmitInteraction}
        onCancelInteraction={vi.fn()}
        onClearStalePermission={vi.fn()}
        onClearStaleInteraction={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("radio", { name: "main" }));

    rerender(
      <PendingActionSurface
        action={{
          kind: "user-question",
          interaction: userQuestion({
            interactionId: "ask-2",
            payload: {
              questions: [
                {
                  header: "Branch",
                  question: "Which branch?",
                  options: [
                    { label: "release", description: "Use release branch" },
                    { label: "hotfix", description: "Use hotfix branch" },
                  ],
                },
              ],
            },
          }),
        }}
        onAllowPermission={vi.fn()}
        onDenyPermission={vi.fn()}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={onSubmitInteraction}
        onCancelInteraction={vi.fn()}
        onClearStalePermission={vi.fn()}
        onClearStaleInteraction={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "继续" })).not.toBeDisabled();

    await user.click(screen.getByRole("radio", { name: "release" }));
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(onSubmitInteraction).toHaveBeenCalledWith("ask-2", {
        answers: { "Which branch?": "release" },
        annotations: {
          userChoiceSummary: "用户回答了补充问题：Which branch? = release",
        },
      }),
    );
  });

  it("guards AskUserQuestion submit against duplicate clicks while in flight", async () => {
    const user = userEvent.setup();
    let resolveSubmit: () => void = () => {};
    const onSubmitInteraction = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveSubmit = resolve;
        }),
    );

    renderSurface(
      { kind: "user-question", interaction: userQuestion() },
      { onSubmitInteraction },
    );

    await user.click(screen.getByRole("radio", { name: "main" }));
    const submitButton = screen.getByRole("button", { name: "继续" });
    await user.dblClick(submitButton);

    expect(onSubmitInteraction).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveSubmit();
    });
  });

  it("uses distinct input names as repeated question text is paged", async () => {
    const user = userEvent.setup();
    const { container } = renderSurface({
      kind: "user-question",
      interaction: userQuestion({
        payload: {
          questions: [
            {
              header: "First",
              question: "Same question?",
              options: [{ label: "A", description: "First answer" }],
            },
            {
              header: "Second",
              question: "Same question?",
              options: [{ label: "B", description: "Second answer" }],
            },
          ],
        },
      }),
    });

    let inputs = Array.from(container.querySelectorAll('input[type="radio"]'));

    expect(inputs).toHaveLength(2);
    expect(inputs[0]).toHaveAttribute("name", expect.stringContaining("-0-"));

    await user.click(screen.getByRole("button", { name: "下一题" }));
    inputs = Array.from(container.querySelectorAll('input[type="radio"]'));

    expect(inputs).toHaveLength(2);
    expect(inputs[0]).toHaveAttribute("name", expect.stringContaining("-1-"));
  });

  it("renders one textarea at a time for repeated question text", async () => {
    const user = userEvent.setup();
    renderSurface({
      kind: "user-question",
      interaction: userQuestion({
        payload: {
          questions: [
            { header: "First", question: "Same question?", options: [] },
            { header: "Second", question: "Same question?", options: [] },
          ],
        },
      }),
    });

    expect(
      screen.getAllByRole("textbox", { name: "Same question?" }),
    ).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "下一题" }));
    expect(
      screen.getAllByRole("textbox", { name: "Same question?" }),
    ).toHaveLength(1);
  });

  it("submits a free text user question answer", async () => {
    const user = userEvent.setup();
    const onSubmitInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      {
        kind: "user-question",
        interaction: userQuestion({
          payload: {
            questions: [
              {
                header: "Details",
                question: "What should I do?",
                options: [],
              },
            ],
          },
        }),
      },
      { onSubmitInteraction },
    );

    await user.type(
      screen.getByRole("textbox", { name: "What should I do?" }),
      "Please continue",
    );
    await user.click(screen.getByRole("button", { name: "继续" }));

    await waitFor(() =>
      expect(onSubmitInteraction).toHaveBeenCalledWith("ask-1", {
        answers: { "What should I do?": "Please continue" },
        annotations: {
          userChoiceSummary:
            "用户回答了补充问题：What should I do? = Please continue",
        },
      }),
    );
  });

  it("cancels a user question", async () => {
    const user = userEvent.setup();
    const onCancelInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      { kind: "user-question", interaction: userQuestion() },
      { onCancelInteraction },
    );

    await user.click(screen.getByRole("button", { name: /忽略/ }));

    await waitFor(() =>
      expect(onCancelInteraction).toHaveBeenCalledWith("ask-1"),
    );
  });

  it("cancels a user question with Escape", async () => {
    const user = userEvent.setup();
    const onCancelInteraction = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      { kind: "user-question", interaction: userQuestion() },
      { onCancelInteraction },
    );

    await user.keyboard("{Escape}");

    await waitFor(() =>
      expect(onCancelInteraction).toHaveBeenCalledWith("ask-1"),
    );
  });

  it("renders stale permission recovery as a stop-only intercepted state", async () => {
    const user = userEvent.setup();
    const onClearStalePermission = vi.fn().mockResolvedValue(undefined);

    renderSurface(
      {
        kind: "stale-permission",
        sessionId: "conv-1",
        toolName: "Glob",
        toolCallId: "tool-stage",
      },
      { onClearStalePermission },
    );

    expect(screen.getByText("需要恢复任务状态")).toBeInTheDocument();
    expect(screen.getByText(/Glob/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "提交" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "拒绝" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "停止任务" }));

    await waitFor(() =>
      expect(onClearStalePermission).toHaveBeenCalledWith("conv-1"),
    );
  });

  it("does not render a countdown", () => {
    renderSurface({ kind: "permission", ask: permissionAsk() });

    expect(screen.queryByText(/倒计时|秒后|timeout/i)).not.toBeInTheDocument();
  });
});
