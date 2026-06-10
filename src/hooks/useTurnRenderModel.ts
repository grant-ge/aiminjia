/**
 * Plan-D: 把扁平 messages + conversationStreamState.toolExecutions
 * 投影为 RenderTurn[]，供 MessageList 渲染。
 */
import { useMemo } from "react";
import type { ReactNode } from "react";

import {
  isFileActionEnabled,
  isPreviewActionEnabledForFile,
  isPreviewableFileType,
} from "@/components/chat/generatedFileActions";
import { useChatStore } from "@/stores/chatStore";
import type { ToolExecution } from "@/stores/streamingStore";
import type {
  FileAction,
  FileAttachment,
  GeneratedFile,
  Message,
  SkillCommandBreadcrumb,
} from "@/types/message";

export interface RenderAiSegment {
  id: string;
  text: string;
  message: Message;
}

export interface RenderToolStep {
  index: number;
  toolCallId: string;
  name: string;
  status: "running" | "done" | "error";
  durationMs?: number;
  inputJson?: string;
  output?: ReactNode;
  /**
   * Live stdout/stderr tail captured while the step is still running.
   * Cleared once the step transitions to done/error (final output takes over).
   * Driven by `tool:progress` events for long-running shell tools.
   */
  progressTail?: string;
  progressTotalBytes?: number;
}

export interface RenderToolGroup {
  status: "running" | "done";
  steps: RenderToolStep[];
  durationMs: number;
}

export interface RenderToolReceiptItem {
  label?: string;
  value: string;
}

export interface RenderToolReceipt {
  kind: "interaction" | "permission";
  status: "submitted" | "approved" | "denied" | "cancelled";
  questionCount?: number;
  items?: RenderToolReceiptItem[];
  summary?: string;
  message?: string;
}

export interface RenderGeneratedFile {
  id: string;
  conversationId: string;
  title: string;
  fileName: string;
  filePath?: string;
  sub: string;
  appName: string;
  fileType?: string;
  actions: FileAction[];
  canPreview: boolean;
  canOpenExternal: boolean;
  canReveal: boolean;
  primaryAction: "preview" | "open";
}

export interface RenderPeerBanner {
  kind: "peer" | "task";
  from: string;
  to?: string;
  body: string;
  summary?: string;
  agent?: string;
  status?: string;
}

export interface RenderCompactBoundary {
  id: string;
  createdAt: string;
  preTokens?: number;
  postTokens?: number;
  tokensSaved?: number;
  messagesSummarized?: number;
}

/**
 * Interleaved render mode block. Walks messages in their original order and
 * produces a flat sequence of text / tool / file blocks. Consumers iterate
 * `blocks` and render each in place, preserving the natural "assistant text
 * → tool → tool result → assistant text → ..." flow that messages.jsonl
 * already records.
 *
 * `toolStep` blocks reference the same `RenderToolStep` object stored in
 * `toolGroup.steps`, so streaming updates that mutate the step (progressTail,
 * status, output) are reflected in both rendering modes without re-derivation.
 */
export type RenderTurnBlock =
  | { kind: "assistantText"; id: string; segment: RenderAiSegment }
  | { kind: "toolStep"; toolCallId: string; step: RenderToolStep }
  | {
      kind: "toolReceipt";
      id: string;
      toolCallId: string;
      receipt: RenderToolReceipt;
      step?: RenderToolStep;
    }
  | { kind: "generatedFile"; id: string; file: RenderGeneratedFile }
  | { kind: "suggestions"; suggestions: string[] }
  | { kind: "teamMarker"; markerKind: "create" | "delete"; toolCallId: string };

export interface RenderTurn {
  compactBoundary?: RenderCompactBoundary;
  userMessage?: {
    id: string;
    text: string;
    createdAt: string;
    commandText?: string;
    skillCommand?: SkillCommandBreadcrumb;
    files?: FileAttachment[];
  };
  aiSegments: RenderAiSegment[];
  toolGroup?: RenderToolGroup;
  /** Flat ordered sequence for interleaved rendering. Built from messages
   *  in their persistence order; mirrors the same data as
   *  `toolGroup` + `aiSegments` + `generatedFiles` + `suggestions`, just
   *  arranged sequentially. */
  blocks: RenderTurnBlock[];
  /** Number of blocks at the start of `blocks` that come from persisted
   *  messages.jsonl. Blocks at indices >= this value are live
   *  toolExecutions added during streaming. MessageList uses this to
   *  position the streaming text bubble BETWEEN persisted blocks and live
   *  tool blocks (so the natural "text → tool" order is preserved during
   *  the current iter's streaming). */
  persistedBlockCount: number;
  generatedFiles: RenderGeneratedFile[];
  suggestions: string[];
  peerBanners: RenderPeerBanner[];
  /**
   * Set when this turn contains a TeamCreate. Tells MessageList to render
   * an in-line TeamProgressBlock (anchor to the team chat drawer) here
   * instead of the raw TeamCreate tool call card.
   */
  teamMarker?: { kind: "create" | "delete"; toolCallId: string };
  /**
   * 这个 turn 是否包含 final assistant message（toolCalls 为空的 assistant
   * message——chat_turn_driver Step 8 用 message_persisted helper emit、
   * tool_calls=None）。流式期间只有 iter messages（toolCalls 非空），
   * isComplete=false；最终消息到达后 isComplete=true，MessageList 在 interleaved
   * 模式下据此把已完成 turn 折叠成聚合视图，只保留最终结果。
   */
  isComplete: boolean;
}

function isCompactBoundaryMessage(message: Message): boolean {
  return message.role === "system" && message.subtype === "compact_boundary";
}

function normalizeCompactBoundaryForRender(
  message: Message,
): RenderCompactBoundary {
  const meta = message.compactMetadata ?? {};
  const tokensSaved =
    meta.tokensSaved ??
    (typeof meta.preTokens === "number" && typeof meta.postTokens === "number"
      ? Math.max(0, meta.preTokens - meta.postTokens)
      : undefined);
  return {
    id: message.id,
    createdAt: message.createdAt,
    preTokens: meta.preTokens,
    postTokens: meta.postTokens,
    tokensSaved,
    messagesSummarized: meta.messagesSummarized,
  };
}

function toolExecStatusToStep(
  s: ToolExecution["status"],
): RenderToolStep["status"] {
  return s === "executing" ? "running" : s === "error" ? "error" : "done";
}

// ── Artifact mark parsing ─────────────────────────────────────────────────

const ARTIFACT_RE = /!\[artifact\]\((.+?)\)/g;

const ARTIFACT_EXT_TO_TYPE: Record<string, GeneratedFile["fileType"]> = {
  md: "markdown",
  markdown: "markdown",
  html: "html",
  json: "json",
  csv: "csv",
  txt: "txt",
  text: "txt",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  bmp: "image",
  svg: "image",
  xlsx: "excel",
  xls: "excel",
  docx: "word",
  doc: "word",
  pptx: "ppt",
  ppt: "ppt",
  pdf: "pdf",
};

interface ParsedArtifactFile {
  filePath: string;
  fileName: string;
  fileType: GeneratedFile["fileType"];
}

function parseArtifactMarks(text: string): {
  cleanedText: string;
  files: ParsedArtifactFile[];
} {
  const files: ParsedArtifactFile[] = [];
  for (const m of text.matchAll(ARTIFACT_RE)) {
    const filePath = m[1].trim();
    const fileName = filePath.split(/[\\/]/).pop() || filePath;
    const ext = fileName.includes(".")
      ? fileName.split(".").pop()?.toLowerCase()
      : undefined;
    const fileType: GeneratedFile["fileType"] =
      (ext ? ARTIFACT_EXT_TO_TYPE[ext] : undefined) ?? "file";
    files.push({ filePath, fileName, fileType });
  }
  const cleanedText = text.replace(ARTIFACT_RE, "");
  return { cleanedText, files };
}

// ── File size & metadata formatting ──────────────────────────────────────

function formatFileSize(bytes: number | undefined): string | null {
  if (bytes == null || bytes <= 0) return null;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function displayFileType(fileType: string | undefined): string | null {
  const value = fileType?.trim().toUpperCase();
  if (!value) return null;
  if (value === "EXCEL") return "XLS";
  return value;
}

function displayFileCategory(category: string | undefined): string | null {
  switch (category) {
    case "report":
      return "报告";
    case "chart":
      return "图表";
    case "data":
      return "数据";
    case "analysis":
      return "分析";
    case "script":
      return "脚本";
    case "temp":
      return "临时";
    default:
      return category || null;
  }
}

function buildGeneratedFileMeta(
  f: GeneratedFile,
  format?: string,
  subtitle?: string,
): string {
  if (subtitle) return subtitle;
  if (f.isDegraded) {
    const actual =
      displayFileType(f.fileType) ?? format?.toUpperCase() ?? "文件";
    const requested = f.requestedFormat?.trim().toUpperCase();
    return requested
      ? `已降级为 ${actual} · 原请求 ${requested}`
      : `已降级为 ${actual}`;
  }

  const parts = [
    formatFileSize(f.fileSize),
    displayFileCategory(f.category),
  ].filter(Boolean);
  return parts.join(" · ");
}

function normalizeGeneratedFile(
  f: GeneratedFile,
  conversationId: string,
): RenderGeneratedFile {
  const anyF = f as unknown as {
    id: string;
    title?: string;
    fileName?: string;
    filePath?: string;
    subtitle?: string;
    appName?: string;
    format?: string;
    fileType?: string;
    actions?: FileAction[];
  };
  const title = anyF.title || anyF.fileName || "未命名文件";
  const fileName = anyF.fileName ?? title;
  const fileType = anyF.fileType;
  const actions = anyF.actions ?? [];
  const canPreviewByType = isPreviewableFileType(fileType, fileName);
  const canPreview =
    canPreviewByType &&
    isPreviewActionEnabledForFile(actions, fileType, fileName);
  const canOpenExternal = isFileActionEnabled(actions, "open");
  const canReveal = isFileActionEnabled(actions, "reveal");
  return {
    id: anyF.id,
    conversationId,
    title,
    fileName,
    filePath: anyF.filePath,
    sub: buildGeneratedFileMeta(f, anyF.format, anyF.subtitle),
    appName: anyF.appName || "打开",
    fileType,
    actions,
    canPreview,
    canOpenExternal,
    canReveal,
    primaryAction: canPreview ? "preview" : "open",
  };
}

function ensureToolGroup(
  turn: RenderTurn,
  status: RenderToolGroup["status"] = "running",
): RenderToolGroup {
  if (!turn.toolGroup) {
    turn.toolGroup = { status, steps: [], durationMs: 0 };
  }
  return turn.toolGroup;
}

function stringifyInput(input: unknown): string | undefined {
  if (input == null) return undefined;
  try {
    return JSON.stringify(input, null, 2);
  } catch {
    return String(input);
  }
}

function truncateOutput(text: string, isError: boolean, maxLines = 20): string {
  const lines = text.split("\n");
  if (lines.length <= maxLines) return text;
  if (isError) {
    return `... (total ${lines.length} lines, truncated)\n${lines.slice(-maxLines).join("\n")}`;
  }
  return `${lines.slice(0, maxLines).join("\n")}\n... (total ${lines.length} lines, truncated)`;
}

function parseQuotedString(
  input: string,
  start: number,
): { value: string; next: number } | null {
  if (input[start] !== '"') return null;
  let escaped = false;
  for (let i = start + 1; i < input.length; i += 1) {
    const ch = input[i];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (ch === "\\") {
      escaped = true;
      continue;
    }
    if (ch === '"') {
      const raw = input.slice(start, i + 1);
      try {
        return { value: JSON.parse(raw), next: i + 1 };
      } catch {
        return { value: raw.slice(1, -1), next: i + 1 };
      }
    }
  }
  return null;
}

function parseAskUserAnswerPairs(text: string): RenderToolReceiptItem[] {
  const items: RenderToolReceiptItem[] = [];
  let i = 0;

  while (i < text.length) {
    while (i < text.length && /[\s,]/.test(text[i] ?? "")) i += 1;
    const label = parseQuotedString(text, i);
    if (!label) break;
    i = label.next;
    while (i < text.length && /\s/.test(text[i] ?? "")) i += 1;
    if (text[i] !== "=") break;
    i += 1;
    while (i < text.length && /\s/.test(text[i] ?? "")) i += 1;

    let value = "";
    const quotedValue = parseQuotedString(text, i);
    if (quotedValue) {
      value = quotedValue.value;
      i = quotedValue.next;
    } else {
      const start = i;
      while (i < text.length && text[i] !== ",") i += 1;
      value = text.slice(start, i).trim();
    }

    const cleanLabel = label.value.trim();
    const cleanValue = value.trim();
    if (cleanLabel || cleanValue) {
      items.push({
        label: cleanLabel || undefined,
        value: cleanValue,
      });
    }
  }

  return items;
}

function compactReceiptText(value: string): string {
  const normalized = value.replace(/\s+/g, " ").trim();
  if (normalized.length <= 42) return normalized;
  return `${normalized.slice(0, 41)}…`;
}

function summarizeReceiptItems(
  items: RenderToolReceiptItem[],
): string | undefined {
  const values = items
    .map((item) => compactReceiptText(item.value))
    .filter(Boolean);

  if (values.length === 0) return undefined;

  const visible = values.slice(0, 6);
  const suffix =
    values.length > visible.length ? ` +${values.length - visible.length}` : "";
  return `${visible.join(" / ")}${suffix}`;
}

type NormalizedToolResult = NonNullable<Message["toolResult"]>;

function normalizeToolResultMessage(
  message: Message,
): NormalizedToolResult | null {
  if (message.role !== "tool") return null;
  if (message.toolResult?.toolCallId) return message.toolResult;

  const raw = message as Message & {
    toolCallId?: string;
    name?: string;
    durationMs?: number;
    content?: Message["content"] & {
      isError?: boolean;
      content?: string;
      result?: string;
    };
  };
  const toolCallId = raw.toolCallId;
  if (!toolCallId) return null;

  const content = raw.content;
  return {
    toolCallId,
    name: raw.name ?? "",
    content: content?.text ?? content?.content ?? content?.result ?? "",
    isError: Boolean(content?.isError),
    durationMs: raw.durationMs,
  };
}

function parseToolReceipt(
  result: NormalizedToolResult,
  questionCountHint?: number,
): RenderToolReceipt | null {
  const content = result.content.trim();
  if (!content) return null;

  if (result.name === "AskUserQuestion") {
    const match = content.match(
      /^User has answered your questions:\s*([\s\S]*?)\.\s*You can now continue/,
    );
    if (match) {
      const items = parseAskUserAnswerPairs(match[1] ?? "");
      const questionCount =
        questionCountHint && questionCountHint > 0
          ? questionCountHint
          : items.length;
      return {
        kind: "interaction",
        status: "submitted",
        questionCount,
        items,
        summary: summarizeReceiptItems(items),
        message: items.length > 0 ? undefined : content,
      };
    }
    if (
      content.includes("用户忽略了这个补充问题") ||
      content.includes("The user ignored this follow-up question")
    ) {
      return {
        kind: "interaction",
        status: "cancelled",
        questionCount: questionCountHint,
      };
    }
  }

  const denied =
    content.includes("用户拒绝了这个权限申请") ||
    content.includes("The user denied this permission request") ||
    content.includes("Permission request denied by user");
  if (denied) {
    return null;
  }

  const cancelled =
    content.includes("用户跳过了这个权限申请") ||
    content.includes("The user skipped this permission request") ||
    content.includes("Permission request cancelled by user");
  if (cancelled) {
    return null;
  }

  const approved =
    content.includes("用户允许了这个权限申请") ||
    content.includes("The user approved this permission request");
  if (approved) {
    return null;
  }

  return null;
}

function recalcToolGroup(group: RenderToolGroup): void {
  group.steps.forEach((step, idx) => {
    step.index = idx + 1;
  });
  group.durationMs = group.steps.reduce(
    (acc, s) => acc + (s.durationMs ?? 0),
    0,
  );
  group.status = group.steps.some((s) => s.status === "running")
    ? "running"
    : "done";
}

function normalizeUserMessageForRender(
  message: Message,
): NonNullable<RenderTurn["userMessage"]> {
  const rawText = message.content.text ?? "";
  const files = message.content.files;
  if (message.content.skillCommand || message.content.commandText) {
    return {
      id: message.id,
      text: rawText,
      createdAt: message.createdAt,
      commandText: message.content.commandText,
      skillCommand: message.content.skillCommand,
      files,
    };
  }

  const slashMatch = rawText.match(
    /^\/([A-Za-z0-9][A-Za-z0-9_-]*)(?:\s+([\s\S]*))?$/,
  );
  if (!slashMatch) {
    return {
      id: message.id,
      text: rawText,
      createdAt: message.createdAt,
      files,
    };
  }

  const skillId = slashMatch[1];
  const text = slashMatch[2]?.trimStart() ?? "";
  const command = `/${skillId}`;
  return {
    id: message.id,
    text,
    createdAt: message.createdAt,
    commandText: rawText,
    skillCommand: { id: skillId, label: skillId, command },
    files,
  };
}

const PEER_MESSAGES_XML_RE = /^<peer-messages\b[\s\S]*<\/peer-messages>$/;
const TASK_NOTIFICATION_XML_RE =
  /^<task-notification\b[\s\S]*<\/task-notification>$/;

function isInternalEventMessage(text: string): boolean {
  const trimmed = text.trim();
  return (
    PEER_MESSAGES_XML_RE.test(trimmed) || TASK_NOTIFICATION_XML_RE.test(trimmed)
  );
}

/**
 * Tool names that are part of the team-chat substrate and should NOT appear
 * in the main conversation's tool-execution panel. They are visible inside
 * the TeamChatDrawer instead. TeamCreate produces an inline anchor block
 * via `teamMarker`; the others are silently hidden.
 */
const TEAM_HIDDEN_TOOLS = new Set([
  "SendMessage",
  "TeamCreate",
  "TeamDelete",
  "TeammateStop",
]);

export function buildTurnsFromMessages(
  messages: Message[],
  toolExecutions: ToolExecution[],
  options: { activeTurn?: boolean } = {},
): RenderTurn[] {
  // 关键：先把所有 tool result 按 toolCallId 索引一遍。
  // 流式过程中 tool:completed 事件比 MessagePersisted(iter assistant) 早到，
  // 而前者会把 tool result 消息推到 messages 数组末尾，后者也是 append——
  // 导致 store 里 tool result 排在 assistant 前。若直接顺序 walk，会先走
  // tool result 的 orphan 分支把工具 block 推进 blocks，然后 assistant 的 text
  // 才补在工具后面，渲染成 [tool, text, tool, text, ...] 反序。
  // 改为：tool result 不再产生独立 block，只作为"输出内容"查询表，由 assistant
  // 的 toolCalls 决定 block 顺序（text → tool → text → tool）。
  const toolResultByCallId = new Map<string, NormalizedToolResult>();
  for (const m of messages) {
    const result = normalizeToolResultMessage(m);
    if (result?.toolCallId) {
      toolResultByCallId.set(result.toolCallId, result);
    }
  }

  const turns: RenderTurn[] = [];
  let current: RenderTurn | null = null;

  for (const m of messages) {
    if (isCompactBoundaryMessage(m)) {
      turns.push({
        compactBoundary: normalizeCompactBoundaryForRender(m),
        userMessage: undefined,
        aiSegments: [],
        toolGroup: undefined,
        blocks: [],
        persistedBlockCount: 0,
        generatedFiles: [],
        suggestions: [],
        peerBanners: [],
        isComplete: true,
      });
      current = null;
      continue;
    }

    if (m.role === "user") {
      if (m.isCompactSummary) {
        continue;
      }
      const text = m.content.text ?? "";
      // Runtime XML is injected as user-role context for the LLM, but it is
      // not user-authored chat content and should not render a visible row.
      if (isInternalEventMessage(text)) {
        continue;
      }
      // Normal user message — original logic.
      current = {
        userMessage: normalizeUserMessageForRender(m),
        aiSegments: [],
        toolGroup: undefined,
        blocks: [],
        persistedBlockCount: 0,
        generatedFiles: [],
        suggestions: [],
        peerBanners: [],
        isComplete: false,
      };
      turns.push(current);
      continue;
    }

    if (!current) {
      current = {
        userMessage: undefined,
        aiSegments: [],
        toolGroup: undefined,
        blocks: [],
        persistedBlockCount: 0,
        generatedFiles: [],
        suggestions: [],
        peerBanners: [],
        isComplete: false,
      };
      turns.push(current);
    }

    if (m.role === "assistant") {
      // Order in blocks: assistant text first (matches natural narration flow),
      // then tool calls below it.
      const hasToolCalls = (m.toolCalls?.length ?? 0) > 0;
      // 一旦遇到 toolCalls 为空的 assistant message，即说明本 turn 已收到
      // chat_turn_driver Step 8 emit 的最终消息——MessageList 据此把交错模式
      // 下已完成的 turn 折叠成聚合视图，只保留这段最终文字。
      if (!hasToolCalls) {
        current.isComplete = true;
      }
      if (m.content.text) {
        const { cleanedText, files } = parseArtifactMarks(m.content.text);
        const displayText = cleanedText.trim();
        if (displayText) {
          const segment: RenderAiSegment = {
            id: m.id,
            text: displayText,
            message:
              displayText === m.content.text
                ? m
                : { ...m, content: { ...m.content, text: displayText } },
          };
          current.aiSegments.push(segment);
          current.blocks.push({ kind: "assistantText", id: m.id, segment });
        }
        for (const f of files) {
          const file = normalizeGeneratedFile(
            {
              id: `artifact-${m.id}-${f.fileName}`,
              fileName: f.fileName,
              filePath: f.filePath,
              fileType: f.fileType,
              fileSize: 0,
              category: "report",
              version: 1,
              isLatest: true,
              createdAt: m.createdAt || new Date().toISOString(),
              description: "",
              actions: [],
            },
            m.conversationId,
          );
          current.generatedFiles.push(file);
          current.blocks.push({ kind: "generatedFile", id: file.id, file });
        }
      }
      if (m.toolCalls?.length) {
        for (const tc of m.toolCalls) {
          // TeamCreate produces an inline anchor block instead of a tool card.
          // teamMarker 字段仍然保留（聚合模式用它把 TeamProgressBlock 渲染在
          // ToolGroupCard 之后），同时 push 一个 teamMarker block，让交错模式
          // 按 message 自然顺序渲染——TeamCreate 通常出现在 turn 早期，
          // 自然位置就比聚合模式强制的"turn 末尾"更贴近时序。
          if (tc.name === "TeamCreate") {
            // 每个 turn 只展示一个团队卡——遇到第一条 TeamCreate 就锚定，
            // 后续重试（比如 #1 失败、#3 成功）不再产生新卡片。
            // teamMarker block 的实际 push 推迟到 turn finalize 阶段，
            // 位置定在"首条无 toolCalls 的 assistant text 紧贴之前"——
            // 这条 text 通常是 LLM 在团队全部就绪后写的"团队已创建，现在
            // 同时召唤..."这类总结性文字，团队卡贴在它前面作为视觉锚点
            // 最合理（早期失败重试 / Agent 调用等 iter 都在折叠块里）。
            if (!current.teamMarker) {
              current.teamMarker = { kind: "create", toolCallId: tc.id };
            }
            continue;
          }
          // TeamDelete / SendMessage / TeammateStop are hidden from the main
          // conversation — they're visible inside the team chat drawer.
          if (TEAM_HIDDEN_TOOLS.has(tc.name)) {
            continue;
          }
          const group = ensureToolGroup(current);
          let step = group.steps.find((s) => s.toolCallId === tc.id);
          if (!step) {
            step = {
              index: group.steps.length + 1,
              toolCallId: tc.id,
              name: tc.name,
              status: "running",
              inputJson: stringifyInput(tc.arguments),
            };
            // 从 tool result 索引里捞输出内容/状态/耗时（如果已经到达）。
            const result = toolResultByCallId.get(tc.id);
            if (result) {
              step.status = result.isError ? "error" : "done";
              step.output = result.content
                ? truncateOutput(result.content, result.isError)
                : undefined;
              step.durationMs = result.durationMs;
            }
            group.steps.push(step);
          } else {
            if (!step.inputJson) {
              step.inputJson = stringifyInput(tc.arguments);
            }
            // 已有 step（比如来自 live toolExecutions），用 tool result 补/覆盖输出。
            const result = toolResultByCallId.get(tc.id);
            if (result) {
              step.status = result.isError ? "error" : "done";
              if (result.content) {
                step.output = truncateOutput(result.content, result.isError);
              }
              if (result.durationMs != null) {
                step.durationMs = result.durationMs;
              }
            }
          }
          // blocks reference the same step object; status/output updates from
          // tool results or live toolExecutions propagate automatically.
          const isAskUserQuestion = tc.name === "AskUserQuestion";
          if (
            !current.blocks.some(
              (b) => b.kind === "toolStep" && b.toolCallId === tc.id,
            )
          ) {
            current.blocks.push({ kind: "toolStep", toolCallId: tc.id, step });
          }
          const result = toolResultByCallId.get(tc.id);
          if (
            !isAskUserQuestion &&
            result &&
            !current.blocks.some(
              (b) => b.kind === "toolReceipt" && b.toolCallId === tc.id,
            )
          ) {
            const receipt = parseToolReceipt(result);
            if (receipt) {
              current.blocks.push({
                kind: "toolReceipt",
                id: `receipt-${tc.id}`,
                toolCallId: tc.id,
                receipt,
              });
            }
          }
        }
      }
      if (m.content.generatedFiles?.length) {
        for (const f of m.content.generatedFiles) {
          const file = normalizeGeneratedFile(f, m.conversationId);
          current.generatedFiles.push(file);
          current.blocks.push({ kind: "generatedFile", id: file.id, file });
        }
      }
      continue;
    }

    // m.role === 'tool' 的消息不再单独产生 block——它们的内容已经通过
    // toolResultByCallId 映射在 assistant toolCalls 处合入对应 step。
    // 这避免了事件到达顺序（tool:completed 早于 MessagePersisted）导致的
    // tool→text 反序问题。
  }

  // 把 teamMarker block 插到"首条无 toolCalls 的 assistant text 紧贴
  // 之前"。这条 text 通常是团队就绪后 LLM 写的"团队已创建..."这类总结，
  // 团队卡作为它的视觉锚点最合理；找不到（流式中 / 无 final text）就
  // append 到 blocks 末尾，避免丢失。
  for (const turn of turns) {
    if (!turn.teamMarker) continue;
    if (turn.blocks.some((b) => b.kind === "teamMarker")) continue;
    const markerBlock: RenderTurnBlock = {
      kind: "teamMarker",
      markerKind: turn.teamMarker.kind,
      toolCallId: turn.teamMarker.toolCallId,
    };
    const anchorIdx = turn.blocks.findIndex(
      (b) => b.kind === "assistantText" && !b.segment.message.toolCalls?.length,
    );
    if (anchorIdx >= 0) {
      turn.blocks.splice(anchorIdx, 0, markerBlock);
    } else {
      turn.blocks.push(markerBlock);
    }
  }

  // Snapshot persisted block counts before live toolExecutions append.
  // MessageList uses this to place streamingContent BETWEEN persisted blocks
  // and live tool blocks for the active iteration.
  for (const turn of turns) {
    turn.persistedBlockCount = turn.blocks.length;
  }

  // Persisted assistant messages can outlive the runtime that created them:
  // stopping a turn after the assistant tool-call snapshot is saved leaves no
  // matching tool-result messages. Those historical snapshots must not render
  // as forever-running after the conversation is no longer active.
  const activeTurn = options.activeTurn ? turns[turns.length - 1] : null;
  for (const turn of turns) {
    if (!turn.toolGroup || turn === activeTurn) continue;
    for (const step of turn.toolGroup.steps) {
      if (
        step.status === "running" &&
        !toolResultByCallId.has(step.toolCallId)
      ) {
        step.status = "done";
      }
    }
  }

  if (toolExecutions.length > 0 && turns.length > 0) {
    const target = turns[turns.length - 1];
    const group = ensureToolGroup(target);
    for (const t of toolExecutions) {
      const existing = group.steps.find((s) => s.toolCallId === t.toolId);
      const output = t.output
        ? truncateOutput(t.output, t.status === "error")
        : undefined;
      // progressTail is meaningful only while the step is still running.
      // Once the step finishes, the final `output` (from tool:completed)
      // takes over so we don't double-render.
      const progressTail =
        t.status === "executing" ? t.progressTail : undefined;
      const progressTotalBytes =
        t.status === "executing" ? t.progressTotalBytes : undefined;
      if (existing) {
        existing.status = toolExecStatusToStep(t.status);
        if (t.durationMs != null) existing.durationMs = t.durationMs;
        if (t.input != null && !existing.inputJson)
          existing.inputJson = stringifyInput(t.input);
        if (output && !existing.output) existing.output = output;
        existing.progressTail = progressTail;
        existing.progressTotalBytes = progressTotalBytes;
      }
      // 注意：不再走 orphan push 分支。chat_turn_driver 已经在 execute_round
      // 之前 persist + emit 了 iter assistant message，所以 message:updated 一定
      // 先于 tool:executing 到达，buildTurnsFromMessages 在 walk persisted
      // messages 时已经把这个 toolCallId 推进 group.steps + blocks（status='running'），
      // 接下来 toolExecutions 事件一定走 existing 分支只更新 status。React key
      // 全程稳定，避免了 live tool → persisted tool 接力导致 unmount/mount。
    }
  }

  for (const turn of turns) {
    if (turn.toolGroup) {
      recalcToolGroup(turn.toolGroup);
    }
  }

  return turns;
}

const EMPTY_TOOL_EXECUTIONS: ToolExecution[] = [];

export function useTurnRenderModel(): RenderTurn[] {
  const messages = useChatStore((s) => s.messages);
  const activeId = useChatStore((s) => s.activeConversationId);
  const activeTurn = useChatStore((s) => {
    if (!activeId) return false;
    return (
      s.busyConversations.has(activeId) ||
      (s.streamStates[activeId]?.isStreaming ?? false)
    );
  });
  const toolExecutions = useChatStore((s) => {
    if (!activeId) return EMPTY_TOOL_EXECUTIONS;
    return s.streamStates[activeId]?.toolExecutions ?? EMPTY_TOOL_EXECUTIONS;
  });
  return useMemo(
    () => buildTurnsFromMessages(messages, toolExecutions, { activeTurn }),
    [messages, toolExecutions, activeTurn],
  );
}
