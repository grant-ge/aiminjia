import { describe, expect, it } from "vitest";

import { buildTurnsFromMessages } from "../useTurnRenderModel";
import type {
  AssistantToolCall,
  GeneratedFile,
  Message,
  ToolResultContent,
} from "@/types/message";
import type { ToolExecution } from "@/stores/streamingStore";

function userMsg(id: string, text: string): Message {
  return {
    id,
    conversationId: "c1",
    role: "user",
    createdAt: new Date().toISOString(),
    content: { text },
  };
}

function aiMsg(id: string, text: string): Message {
  return {
    id,
    conversationId: "c1",
    role: "assistant",
    createdAt: new Date().toISOString(),
    content: { text },
  };
}

function assistantMsgWithToolCalls(
  id: string,
  toolCalls: AssistantToolCall[],
): Message {
  return {
    id,
    conversationId: "c1",
    role: "assistant",
    createdAt: new Date().toISOString(),
    content: { text: "" },
    toolCalls,
  };
}

function persistedToolMsg(
  id: string,
  {
    toolCallId,
    name,
    content,
    isError = false,
  }: {
    toolCallId: string;
    name: string;
    content: string;
    isError?: boolean;
  },
): Message {
  return {
    id,
    conversationId: "c1",
    role: "tool",
    createdAt: new Date().toISOString(),
    content: { text: content, isError } as Message["content"],
    toolCallId,
    name,
  } as Message;
}

function toolResultMsg(id: string, toolResult: ToolResultContent): Message {
  return {
    id,
    conversationId: "c1",
    role: "tool",
    createdAt: new Date().toISOString(),
    content: { text: "" },
    toolResult,
  };
}

function compactBoundaryMsg(id: string): Message {
  return {
    id,
    conversationId: "c1",
    role: "system",
    createdAt: new Date().toISOString(),
    content: { text: "Conversation compacted" },
    subtype: "compact_boundary",
    compactMetadata: {
      preTokens: 12000,
      postTokens: 4500,
      tokensSaved: 7500,
      messagesSummarized: 18,
    },
  } as Message;
}

describe("buildTurnsFromMessages", () => {
  it("marks a completed tool turn collapsible around its final assistant answer", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "检查项目"),
        aiMsg("a1", "我先看一下文件。"),
        assistantMsgWithToolCalls("a2", [
          { id: "tc-1", name: "Read", arguments: { file_path: "README.md" } },
        ]),
        toolResultMsg("t1", {
          toolCallId: "tc-1",
          name: "Read",
          content: "README contents",
          isError: false,
        }),
        aiMsg("a3", "已检查 README，并确认项目启动方式。"),
      ],
      [],
    );

    expect(turns[0].completedFinalAnswer).toMatchObject({
      id: "a3",
      text: "已检查 README，并确认项目启动方式。",
    });
    expect(turns[0].shouldCollapseCompletedProcess).toBe(true);
  });

  it("does not collapse a simple completed chat turn without foldable process", () => {
    const turns = buildTurnsFromMessages(
      [userMsg("u1", "你好"), aiMsg("a1", "你好！")],
      [],
    );

    expect(turns[0].completedFinalAnswer).toMatchObject({
      id: "a1",
      text: "你好！",
    });
    expect(turns[0].shouldCollapseCompletedProcess).toBe(false);
  });

  it("does not collapse an interrupted tool turn without a final assistant answer", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "检查项目"),
        aiMsg("a1", "我先看一下文件。"),
        assistantMsgWithToolCalls("a2", [
          { id: "tc-1", name: "Read", arguments: { file_path: "README.md" } },
        ]),
        toolResultMsg("t1", {
          toolCallId: "tc-1",
          name: "Read",
          content: "cancelled",
          isError: true,
        }),
      ],
      [],
    );

    expect(turns[0].completedFinalAnswer).toBeUndefined();
    expect(turns[0].shouldCollapseCompletedProcess).toBe(false);
  });

  it("keeps assistant replies to internal task notifications separate from the previous visible turn", () => {
    const taskNotification = userMsg(
      "notify-1",
      [
        "<task-notification>",
        "  <task-id>bg-1</task-id>",
        "  <status>completed</status>",
        "  <summary>Background command completed</summary>",
        "</task-notification>",
      ].join("\n"),
    );

    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "请执行一个会等待 30 秒的命令。"),
        assistantMsgWithToolCalls("a1", [
          { id: "tc-1", name: "Bash", arguments: { command: "sleep 30" } },
        ]),
        toolResultMsg("t1", {
          toolCallId: "tc-1",
          name: "Bash",
          content: '{"status":"backgrounded"}',
          isError: false,
        }),
        aiMsg("a2", "30 秒命令已在后台执行。"),
        userMsg("u2", "请创建两个文件并三点总结。"),
        assistantMsgWithToolCalls("a3", [
          { id: "tc-2", name: "Bash", arguments: { command: "touch /tmp/a" } },
        ]),
        toolResultMsg("t2", {
          toolCallId: "tc-2",
          name: "Bash",
          content: "",
          isError: false,
        }),
        aiMsg("a4", "三点总结：两个文件已创建并检查。"),
        taskNotification,
        aiMsg("a5", "之前提交的 30 秒等待命令已执行完毕，正常退出。"),
      ],
      [],
    );

    expect(turns).toHaveLength(3);
    expect(turns[1].userMessage?.id).toBe("u2");
    expect(turns[1].completedFinalAnswer).toMatchObject({
      id: "a4",
      text: "三点总结：两个文件已创建并检查。",
    });
    expect(turns[2].userMessage).toBeUndefined();
    expect(turns[2].completedFinalAnswer).toMatchObject({
      id: "a5",
      text: "之前提交的 30 秒等待命令已执行完毕，正常退出。",
    });
    expect(turns[2].shouldCollapseCompletedProcess).toBe(false);
  });

  it("groups messages into turns starting at each user message", () => {
    const msgs = [
      userMsg("u1", "hi"),
      aiMsg("a1", "hello"),
      userMsg("u2", "again"),
      aiMsg("a2", "hi!"),
    ];
    const turns = buildTurnsFromMessages(msgs, []);
    expect(turns.map((t) => t.userMessage?.id)).toEqual(["u1", "u2"]);
    expect(turns[0].aiSegments.map((s) => s.id)).toEqual(["a1"]);
    expect(turns[1].aiSegments.map((s) => s.id)).toEqual(["a2"]);
  });

  it("renders compact boundary system messages as boundary blocks", () => {
    const turns = buildTurnsFromMessages(
      [compactBoundaryMsg("compact-1"), userMsg("u1", "next question")],
      [],
    );

    expect(turns[0]?.compactBoundary).toMatchObject({
      id: "compact-1",
      preTokens: 12000,
      postTokens: 4500,
      tokensSaved: 7500,
      messagesSummarized: 18,
    });
    expect(turns[1]?.userMessage?.id).toBe("u1");
  });

  it("hides compact summary user artifacts after compact boundaries", () => {
    const summary = userMsg(
      "summary-1",
      "<context>\nold history summary\n</context>",
    );
    summary.isCompactSummary = true;

    const turns = buildTurnsFromMessages(
      [
        compactBoundaryMsg("compact-1"),
        summary,
        userMsg("u1", "next question"),
        aiMsg("a1", "next answer"),
      ],
      [],
    );

    expect(turns.map((turn) => turn.userMessage?.id).filter(Boolean)).toEqual([
      "u1",
    ]);
    expect(turns.some((turn) => turn.userMessage?.id === "summary-1")).toBe(
      false,
    );
    expect(turns[0]?.compactBoundary?.id).toBe("compact-1");
    expect(turns[1]?.aiSegments.map((segment) => segment.id)).toEqual(["a1"]);
  });

  it("filters task notification XML user messages from the chat turns", () => {
    const notification = userMsg(
      "task-notification",
      [
        "<task-notification>",
        "  <task-id>b7a56f590</task-id>",
        "  <status>completed</status>",
        "  <summary>Background command completed</summary>",
        "</task-notification>",
      ].join("\n"),
    );
    const msgs = [notification, userMsg("u1", "好了"), aiMsg("a1", "登录成功")];

    const turns = buildTurnsFromMessages(msgs, []);

    expect(turns.map((t) => t.userMessage?.id)).toEqual(["u1"]);
    expect(turns[0].userMessage?.text).toBe("好了");
    expect(turns[0].aiSegments.map((s) => s.id)).toEqual(["a1"]);
  });

  it("attaches tool executions to the last turn as a single ToolGroup", () => {
    const msgs = [
      userMsg("u1", "x"),
      assistantMsgWithToolCalls("a1", [
        { id: "t1", name: "fetch_feedback", arguments: {} },
        { id: "t2", name: "cluster_topics", arguments: {} },
      ]),
    ];
    const tools: ToolExecution[] = [
      { toolId: "t1", toolName: "fetch_feedback", status: "completed" },
      { toolId: "t2", toolName: "cluster_topics", status: "completed" },
    ];
    const turns = buildTurnsFromMessages(msgs, tools);
    expect(turns[0].toolGroup).toBeDefined();
    expect(turns[0].toolGroup?.steps.map((s) => s.name)).toEqual([
      "fetch_feedback",
      "cluster_topics",
    ]);
    expect(turns[0].toolGroup?.status).toBe("done");
    // durationMs is 0 when no timestamps are available (test fixtures have no startedAt)
    expect(turns[0].toolGroup?.durationMs).toBe(0);
  });

  it("marks toolGroup as running when any tool is executing", () => {
    const msgs = [
      userMsg("u1", "x"),
      assistantMsgWithToolCalls("a1", [
        { id: "t1", name: "fetch", arguments: {} },
        { id: "t2", name: "run", arguments: {} },
      ]),
    ];
    const tools: ToolExecution[] = [
      { toolId: "t1", toolName: "fetch", status: "completed" },
      { toolId: "t2", toolName: "run", status: "executing" },
    ];
    const turns = buildTurnsFromMessages(msgs, tools);
    expect(turns[0].toolGroup?.status).toBe("running");
  });

  it("does not render abandoned persisted tool calls as running after the turn is inactive", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "inspect files"),
        assistantMsgWithToolCalls("a1", [
          { id: "tc-1", name: "Glob", arguments: { pattern: "**/*.md" } },
          { id: "tc-2", name: "Glob", arguments: { pattern: "**/*.json" } },
        ]),
      ],
      [],
    );

    expect(turns[0].toolGroup?.status).toBe("done");
    expect(turns[0].toolGroup?.steps.map((s) => s.status)).toEqual([
      "done",
      "done",
    ]);
  });

  it("keeps missing tool results running while the latest turn is still active", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "inspect files"),
        assistantMsgWithToolCalls("a1", [
          { id: "tc-1", name: "Glob", arguments: { pattern: "**/*.md" } },
        ]),
      ],
      [],
      { activeTurn: true },
    );

    expect(turns[0].toolGroup?.status).toBe("running");
    expect(turns[0].toolGroup?.steps[0]?.status).toBe("running");
  });

  it("does not keep older missing tool results running when a later turn is active", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "inspect files"),
        assistantMsgWithToolCalls("a1", [
          { id: "tc-1", name: "Glob", arguments: { pattern: "**/*.md" } },
        ]),
        userMsg("u2", "new task"),
        assistantMsgWithToolCalls("a2", [
          { id: "tc-2", name: "Read", arguments: { file_path: "/tmp/a.txt" } },
        ]),
      ],
      [],
      { activeTurn: true },
    );

    expect(turns[0].toolGroup?.status).toBe("done");
    expect(turns[0].toolGroup?.steps[0]?.status).toBe("done");
    expect(turns[1].toolGroup?.status).toBe("running");
    expect(turns[1].toolGroup?.steps[0]?.status).toBe("running");
  });

  it("aiSegment carries the full message object", () => {
    const msg = aiMsg("a1", "hello");
    const turns = buildTurnsFromMessages([userMsg("u1", "hi"), msg], []);
    expect(turns[0].aiSegments[0].message).toStrictEqual(msg);
    expect(turns[0].aiSegments[0].id).toBe("a1");
  });

  it("maps inputJson from assistant.toolCalls by toolCallId", () => {
    const msgs = [
      userMsg("u1", "go"),
      assistantMsgWithToolCalls("a1", [
        { id: "tc-1", name: "run_python", arguments: { code: "print(1)" } },
      ]),
      toolResultMsg("t1", {
        toolCallId: "tc-1",
        name: "run_python",
        content: "1\n",
        isError: false,
      }),
    ];
    const turns = buildTurnsFromMessages(msgs, []);
    const step = turns[0].toolGroup?.steps[0];
    expect(step?.toolCallId).toBe("tc-1");
    expect(step?.inputJson).toContain("print(1)");
    expect(step?.output).toContain("1");
  });

  it("backfills inputJson when tool result arrives before assistant.toolCalls snapshot", () => {
    const msgs = [
      userMsg("u1", "go"),
      toolResultMsg("t1", {
        toolCallId: "tc-1",
        name: "run_python",
        content: "1\n",
        isError: false,
      }),
      assistantMsgWithToolCalls("a1", [
        { id: "tc-1", name: "run_python", arguments: { code: "print(1)" } },
      ]),
    ];
    const step = buildTurnsFromMessages(msgs, [])[0].toolGroup?.steps[0];
    expect(step?.inputJson).toContain("print(1)");
    expect(step?.output).toContain("1");
  });

  it("does not confuse same-name tools called twice", () => {
    const msgs = [
      userMsg("u1", "go"),
      assistantMsgWithToolCalls("a1", [
        { id: "tc-1", name: "browse", arguments: { url: "http://a.com" } },
        { id: "tc-2", name: "browse", arguments: { url: "http://b.com" } },
      ]),
      toolResultMsg("t1", {
        toolCallId: "tc-1",
        name: "browse",
        content: "page A",
        isError: false,
      }),
      toolResultMsg("t2", {
        toolCallId: "tc-2",
        name: "browse",
        content: "page B",
        isError: false,
      }),
    ];
    const steps = buildTurnsFromMessages(msgs, [])[0].toolGroup?.steps ?? [];
    expect(steps).toHaveLength(2);
    expect(steps.find((s) => s.toolCallId === "tc-1")?.output).toContain(
      "page A",
    );
    expect(steps.find((s) => s.toolCallId === "tc-2")?.output).toContain(
      "page B",
    );
  });

  it("error output preserved in step output", () => {
    const msgs = [
      userMsg("u1", "go"),
      assistantMsgWithToolCalls("a1", [
        { id: "tc-1", name: "run_python", arguments: {} },
      ]),
      toolResultMsg("t1", {
        toolCallId: "tc-1",
        name: "run_python",
        content: "Traceback...\nValueError: bad",
        isError: true,
      }),
    ];
    const step = buildTurnsFromMessages(msgs, [])[0].toolGroup?.steps[0];
    expect(step?.status).toBe("error");
    expect(step?.output).toBeDefined();
  });

  it("keeps answered AskUserQuestion in the tool step stream with input and output", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "plan a trip"),
        assistantMsgWithToolCalls("a1", [
          {
            id: "ask-1",
            name: "AskUserQuestion",
            arguments: {
              questions: [{ question: "预算范围" }, { question: "旅行时长" }],
            },
          },
        ]),
        toolResultMsg("t1", {
          toolCallId: "ask-1",
          name: "AskUserQuestion",
          content:
            'User has answered your questions: "预算范围"="3000-6000", "旅行时长"="3-4 天". You can now continue with the user\'s answers in mind.',
          isError: false,
        }),
      ],
      [],
    );

    expect(turns[0].blocks.map((b) => b.kind)).toEqual(["toolStep"]);
    const step = turns[0].toolGroup?.steps.find(
      (s) => s.toolCallId === "ask-1",
    );
    expect(step).toMatchObject({
      name: "AskUserQuestion",
      status: "done",
    });
    expect(step?.inputJson).toContain("预算范围");
    expect(step?.inputJson).toContain("旅行时长");
    expect(step?.output).toContain("3000-6000");
    expect(step?.output).toContain("3-4 天");
  });

  it("keeps long AskUserQuestion answers inside the tool step output", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "写科幻小说"),
        assistantMsgWithToolCalls("a1", [
          {
            id: "ask-1",
            name: "AskUserQuestion",
            arguments: {
              questions: [
                { question: "你的科幻小说想要围绕哪个核心科学概念展开？" },
                { question: "你的目标读者群体是谁？" },
              ],
            },
          },
        ]),
        toolResultMsg("t1", {
          toolCallId: "ask-1",
          name: "AskUserQuestion",
          content:
            'User has answered your questions: "你的科幻小说想要围绕哪个核心科学概念展开？三体用了「三体问题 + 黑暗森林法则」，你的故事想以什么样的科学点子或理论作为基石？"="计算科学，AI 的边界", "你的目标读者群体是谁？这会影响故事的科学深度和语言风格。"="硬核科幻迷". You can now continue with the user\'s answers in mind.',
          isError: false,
        }),
      ],
      [],
    );

    const step = turns[0].toolGroup?.steps.find(
      (s) => s.toolCallId === "ask-1",
    );
    expect(step?.output).toContain("计算科学，AI 的边界");
    expect(step?.output).toContain("硬核科幻迷");
  });

  it("renders AskUserQuestion from persisted messages.jsonl tool shape as a tool step trace", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "问我 3 个问题"),
        assistantMsgWithToolCalls("a1", [
          {
            id: "ask-1",
            name: "AskUserQuestion",
            arguments: {
              questions: [
                { question: "格式" },
                { question: "类型" },
                { question: "风格" },
              ],
            },
          },
        ]),
        persistedToolMsg("t1", {
          toolCallId: "ask-1",
          name: "AskUserQuestion",
          content:
            'User has answered your questions: "你希望我输出的报告/产物通常用什么格式？"="Excel", "你日常工作中最常处理哪种类型的数据？"="财务报表/预算", "你更倾向于我给出结论性的建议，还是详细的分析过程？"="结论优先". You can now continue with the user\'s answers in mind.',
        }),
      ],
      [],
    );

    expect(turns[0].blocks.map((b) => b.kind)).toEqual(["toolStep"]);
    const step = turns[0].toolGroup?.steps.find(
      (s) => s.toolCallId === "ask-1",
    );
    expect(step?.inputJson).toContain("格式");
    expect(step?.output).toContain("Excel");
    expect(step?.output).toContain("财务报表/预算");
    expect(step?.output).toContain("结论优先");
  });

  it("keeps ignored AskUserQuestion feedback inside the tool step output", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "写科幻小说"),
        assistantMsgWithToolCalls("a1", [
          {
            id: "ask-1",
            name: "AskUserQuestion",
            arguments: { questions: [] },
          },
        ]),
        toolResultMsg("t1", {
          toolCallId: "ask-1",
          name: "AskUserQuestion",
          content:
            "用户忽略了这个补充问题。请基于已有信息继续；不要输出空标题、空书名号或空占位符，缺少关键名称时请使用通用名称。",
          isError: true,
        }),
      ],
      [],
    );

    const step = turns[0].toolGroup?.steps.find(
      (s) => s.toolCallId === "ask-1",
    );
    expect(step?.status).toBe("error");
    expect(step?.output).toContain("用户忽略了这个补充问题");
  });

  it("does not render visible receipt blocks for denied permission tool results", () => {
    const turns = buildTurnsFromMessages(
      [
        userMsg("u1", "read a file"),
        assistantMsgWithToolCalls("a1", [
          {
            id: "read-1",
            name: "Read",
            arguments: { file_path: "/private/tmp/secret.txt" },
          },
        ]),
        toolResultMsg("t1", {
          toolCallId: "read-1",
          name: "Read",
          content:
            "用户拒绝了这个权限申请，并给出调整说明：请改用工作区里的摘要文件。",
          isError: true,
        }),
      ],
      [],
    );

    const receipt = turns[0].blocks.find((b) => b.kind === "toolReceipt");
    expect(receipt).toBeUndefined();
  });

  it("preserves skill command metadata on user messages for the chat-scene UI", () => {
    const msg: Message = {
      ...userMsg("u1", "你可以做什么"),
      content: {
        text: "你可以做什么",
        commandText: "/salary-query 你可以做什么",
        skillCommand: {
          id: "salary-query",
          label: "salary-query",
          command: "/salary-query",
        },
      },
    };

    const turns = buildTurnsFromMessages([msg], []);

    expect(turns[0].userMessage).toMatchObject({
      id: "u1",
      text: "你可以做什么",
      commandText: "/salary-query 你可以做什么",
      skillCommand: {
        id: "salary-query",
        label: "salary-query",
        command: "/salary-query",
      },
    });
  });

  it("formats generated file metadata for the compact file card subtitle", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        generatedFiles: [
          {
            id: "file-1",
            fileName: "mock-data-matrix.csv",
            filePath: "/tmp/mock-data-matrix.csv",
            fileType: "csv",
            fileSize: 12_288,
            category: "data",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "exported matrix",
            actions: [],
          },
        ],
      },
    };

    const turns = buildTurnsFromMessages([userMsg("u1", "export"), msg], []);

    expect(turns[0].generatedFiles[0]).toMatchObject({
      title: "mock-data-matrix.csv",
      sub: "12 KB · 数据",
      appName: "打开",
    });
  });

  it("uses degradation notice as generated file metadata when available", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        generatedFiles: [
          {
            id: "file-1",
            fileName: "report.html",
            filePath: "/tmp/report.html",
            fileType: "html",
            fileSize: 2048,
            category: "report",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "",
            actions: [],
            isDegraded: true,
            requestedFormat: "docx",
          },
        ],
      },
    };

    const turns = buildTurnsFromMessages([userMsg("u1", "report"), msg], []);

    expect(turns[0].generatedFiles[0].sub).toBe("已降级为 HTML · 原请求 DOCX");
  });

  it("normalizes slash command user text into skill command metadata", () => {
    const turns = buildTurnsFromMessages(
      [userMsg("u1", "/salary-query 看看你的技能能力")],
      [],
    );

    expect(turns[0].userMessage).toMatchObject({
      id: "u1",
      text: "看看你的技能能力",
      commandText: "/salary-query 看看你的技能能力",
      skillCommand: {
        id: "salary-query",
        label: "salary-query",
        command: "/salary-query",
      },
    });
  });

  it("preserves generated file action metadata for file card interactions", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-1",
            fileName: "report.md",
            filePath: "/tmp/report.md",
            fileType: "markdown",
            fileSize: 128,
            category: "report",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Report",
            actions: [{ type: "preview", label: "Preview", enabled: true }],
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        id: "file-1",
        title: "report.md",
        fileType: "markdown",
        actions: [{ type: "preview", label: "Preview", enabled: true }],
        canPreview: true,
        canOpenExternal: false,
        primaryAction: "preview",
        conversationId: "c1",
      }),
    );
  });

  it("uses safe defaults for old generated file records without actions", () => {
    const oldFile = {
      id: "file-2",
      fileName: "book.xlsx",
      filePath: "/tmp/book.xlsx",
      fileType: "xlsx",
      fileSize: 256,
      category: "legacy-output",
      version: 1,
      isLatest: true,
      createdAt: "2026-04-28T00:00:00Z",
      description: "Workbook",
    } satisfies GeneratedFile;
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: { text: "done", generatedFiles: [oldFile] },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        id: "file-2",
        title: "book.xlsx",
        fileType: "xlsx",
        actions: [],
        canPreview: false,
        canOpenExternal: true,
        primaryAction: "open",
      }),
    );
  });

  it("uses type-based preview even when preview action is disabled", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-3",
            fileName: "report.md",
            filePath: "/tmp/report.md",
            fileType: "markdown",
            fileSize: 128,
            category: "report",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Report",
            actions: [
              { type: "preview", label: "Preview", enabled: false },
              { type: "open", label: "Open", enabled: false },
            ],
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: true,
        canOpenExternal: false,
        primaryAction: "preview",
      }),
    );
  });

  it("marks external open unavailable for non-previewable generated files with disabled open action", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-5",
            fileName: "book.xlsx",
            filePath: "/tmp/book.xlsx",
            fileType: "xlsx",
            fileSize: 256,
            category: "workbook",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Workbook",
            actions: [{ type: "open", label: "Open", enabled: false }],
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: false,
        canOpenExternal: false,
        primaryAction: "open",
      }),
    );
  });

  it("uses image preview when legacy actions omit the preview action", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-legacy-image",
            fileName: "mock-status-chart.png",
            filePath: "/tmp/mock-status-chart.png",
            fileType: "png",
            fileSize: 68,
            category: "chart",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Chart",
            actions: [
              { type: "open", label: "Open", enabled: true },
              { type: "reveal", label: "Open Folder", enabled: true },
            ],
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: true,
        canOpenExternal: true,
        primaryAction: "preview",
      }),
    );
  });

  it("uses structured generatedFiles instead of artifact markers for generated outputs", () => {
    const generatedFile = {
      id: "file-image",
      fileName: "image-task-imgtask_lreq_1781059192913657260-1.png",
      filePath:
        "/Users/oayzz/.renlijia/users/t_1__u_2/conversations/conv-1/generated/images/image-task-imgtask_lreq_1781059192913657260-1.png",
      fileType: "png",
      fileSize: 520553,
      category: "image",
      version: 1,
      isLatest: true,
      createdAt: "2026-06-10T00:00:00Z",
      description: "generated image",
      actions: [],
    } satisfies GeneratedFile;
    const msg: Message = {
      ...aiMsg(
        "a1",
        "done\n\n![artifact](/Users/oayzz/.renlijia/generated/images/image-task-imgtask_lreq_1781059192913657260-1.png)",
      ),
      content: {
        text: "done\n\n![artifact](/Users/oayzz/.renlijia/generated/images/image-task-imgtask_lreq_1781059192913657260-1.png)",
        generatedFiles: [generatedFile],
      },
    };

    const turn = buildTurnsFromMessages([userMsg("u1", "go"), msg], [])[0];

    expect(turn.generatedFiles).toHaveLength(1);
    expect(turn.generatedFiles[0]).toEqual(
      expect.objectContaining({
        id: "file-image",
        filePath: generatedFile.filePath,
      }),
    );
    expect(turn.blocks.filter((block) => block.kind === "generatedFile")).toHaveLength(1);
  });

  it("keeps artifact cards in the marker's original message position", () => {
    const turn = buildTurnsFromMessages(
      [
        userMsg("u1", "make a report"),
        aiMsg(
          "a1",
          "前面这段解释。\n\n![artifact](/tmp/report.md)\n\n后面继续解释。",
        ),
      ],
      [],
    )[0];

    expect(turn.blocks.map((block) => block.kind)).toEqual([
      "assistantText",
      "generatedFile",
      "assistantText",
    ]);
    expect(
      turn.blocks[0].kind === "assistantText" ? turn.blocks[0].segment.text : "",
    ).toBe("前面这段解释。");
    expect(
      turn.blocks[1].kind === "generatedFile" ? turn.blocks[1].file.filePath : "",
    ).toBe("/tmp/report.md");
    expect(
      turn.blocks[2].kind === "assistantText" ? turn.blocks[2].segment.text : "",
    ).toBe("后面继续解释。");
  });

  it("does not render artifact cards for markers inside code spans or code blocks", () => {
    const text = [
      "行内代码 `![artifact](/tmp/inline.md)` 不应该渲染。",
      "",
      "```",
      "![artifact](/tmp/block.md)",
      "```",
    ].join("\n");
    const turn = buildTurnsFromMessages(
      [userMsg("u1", "explain marker"), aiMsg("a1", text)],
      [],
    )[0];

    expect(turn.generatedFiles).toHaveLength(0);
    expect(turn.blocks.map((block) => block.kind)).toEqual(["assistantText"]);
    expect(turn.aiSegments[0].text).toBe(text);
  });

  it("uses type-based preview for legacy HTML actions that omit preview", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-legacy-html",
            fileName: "mock-coverage-report.html",
            filePath: "/tmp/mock-coverage-report.html",
            fileType: "html",
            fileSize: 8971,
            category: "report",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Report",
            actions: [
              { type: "open", label: "Open", enabled: true },
              { type: "reveal", label: "Open Folder", enabled: true },
            ],
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: true,
        primaryAction: "preview",
      }),
    );
  });

  it("marks PNG generated artifacts as previewable in the app", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-image",
            fileName: "mock-status-chart.png",
            filePath: "/tmp/mock-status-chart.png",
            fileType: "png",
            fileSize: 68,
            category: "chart",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Chart",
            actions: [{ type: "preview", label: "Preview", enabled: true }],
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        id: "file-image",
        title: "mock-status-chart.png",
        fileType: "png",
        canPreview: true,
        primaryAction: "preview",
      }),
    );
  });

  it("keeps generated file display title while using fileName as preview fallback", () => {
    const msg: Message = {
      ...aiMsg("a1", "done"),
      content: {
        text: "done",
        generatedFiles: [
          {
            id: "file-4",
            title: "Readable Report",
            fileName: "report.md",
            filePath: "/tmp/report.md",
            fileSize: 128,
            category: "report",
            version: 1,
            isLatest: true,
            createdAt: "2026-04-28T00:00:00Z",
            description: "Report",
          },
        ],
      },
    };

    const generatedFile = buildTurnsFromMessages(
      [userMsg("u1", "go"), msg],
      [],
    )[0].generatedFiles[0];

    expect(generatedFile).toEqual(
      expect.objectContaining({
        title: "Readable Report",
        fileName: "report.md",
        canPreview: true,
        primaryAction: "preview",
      }),
    );
  });
});
