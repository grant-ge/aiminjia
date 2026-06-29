import {
  AlertCircle,
  Boxes,
  ChevronDown,
  ChevronRight,
  FileText,
  MessageCircleQuestion,
  Pencil,
  Search,
  Terminal,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ToolStepRow } from "./ToolStepRow";
import {
  isUserInteractionTool,
  summarizeToolSteps,
  type BucketCount,
  type ToolBucket,
} from "./toolStepSummary";
import type { RenderToolStep } from "@/hooks/useTurnRenderModel";
import { useDevSettingsStore } from "@/stores/devSettingsStore";
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

const MIN_RUNNING_DISPLAY_MS = 800;

const BUCKET_ICON: Record<ToolBucket, LucideIcon> = {
  file_read: FileText,
  command: Terminal,
  search: Search,
  file_edit: Pencil,
  interaction: MessageCircleQuestion,
  mcp: Boxes,
  other: Wrench,
};

/**
 * 给每个 step 套"running 状态最小持续时间"——本地工具瞬间完成
 * (Read/Grep < 100ms) 会导致 loading 一闪而过看不清。后端真实切到 done/error
 * 时如果该 step 的 running 时长未满 minMs，UI 上延迟到 minMs 才切。
 *
 * 历史会话加载（一开始就是 done，没经过 running）不延迟，直接显示真实状态。
 */
function useDelayedSteps(
  steps: readonly RenderToolStep[],
  minMs: number,
): RenderToolStep[] {
  const startTimesRef = useRef<Map<string, number>>(new Map());
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    const now = Date.now();
    const timers: ReturnType<typeof setTimeout>[] = [];
    let nextPending = pendingIds;
    let changed = false;

    for (const step of steps) {
      const id = step.toolCallId;
      if (step.status === "running") {
        if (!startTimesRef.current.has(id)) startTimesRef.current.set(id, now);
        if (nextPending.has(id)) {
          if (!changed) {
            nextPending = new Set(nextPending);
            changed = true;
          }
          nextPending.delete(id);
        }
        continue;
      }
      const startedAt = startTimesRef.current.get(id);
      if (startedAt == null) continue; // 从未 running（历史加载），不延迟
      const elapsed = now - startedAt;
      if (elapsed >= minMs) {
        startTimesRef.current.delete(id);
        if (nextPending.has(id)) {
          if (!changed) {
            nextPending = new Set(nextPending);
            changed = true;
          }
          nextPending.delete(id);
        }
        continue;
      }
      // running 时长不足 minMs，需要延迟显示 done
      if (!nextPending.has(id)) {
        if (!changed) {
          nextPending = new Set(nextPending);
          changed = true;
        }
        nextPending.add(id);
        timers.push(
          setTimeout(() => {
            startTimesRef.current.delete(id);
            setPendingIds((curr) => {
              if (!curr.has(id)) return curr;
              const next = new Set(curr);
              next.delete(id);
              return next;
            });
          }, minMs - elapsed),
        );
      }
    }

    if (changed) setPendingIds(nextPending);
    return () => timers.forEach(clearTimeout);
    // pendingIds 不进 dep——它由当前 effect 自己维护，进 dep 会无限循环。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [steps, minMs]);

  return useMemo(() => {
    if (pendingIds.size === 0) return steps as RenderToolStep[];
    return steps.map((step) =>
      pendingIds.has(step.toolCallId)
        ? { ...step, status: "running" as const }
        : step,
    );
  }, [steps, pendingIds]);
}

interface ToolStepGroupBlockProps {
  steps: readonly RenderToolStep[];
}

/**
 * 一级折叠容器：把连续工具调用合并为一行 Codex 风格摘要
 * （"读取了 3 个文件、运行了 2 个命令 ›"）。点击展开后渲染 N 个 ToolStepRow，
 * 每行再各自可二级展开为输入/输出详情。无 border / 无 bg / 无 shadow。
 */
export function ToolStepGroupBlock({ steps }: ToolStepGroupBlockProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const showToolErrorIcon = useDevSettingsStore((s) => s.showToolErrorIcon);
  const displayedSteps = useDelayedSteps(steps, MIN_RUNNING_DISPLAY_MS);

  if (displayedSteps.length === 0) return null;

  const summary = summarizeToolSteps(displayedSteps);
  const interactionSummaries = displayedSteps
    .filter(
      (step): step is RenderToolStep & { output: string } =>
        isUserInteractionTool(step.name) && typeof step.output === "string",
    )
    .map((step) => summarizeInteractionOutput(step.output))
    .filter((value): value is string => Boolean(value));
  const separator = t("toolGroupSummary.separator");
  const bucketText = summary.buckets
    .map((b) => renderBucket(t, b))
    .join(separator);
  let text = bucketText;
  if (summary.runningCount > 0) {
    text += t("toolGroupSummary.runningSuffix");
  }
  if (summary.errorCount > 0 && showToolErrorIcon) {
    text +=
      separator +
      t("toolGroupSummary.failedSuffix", { count: summary.errorCount });
  }

  const leadingIcon =
    summary.runningCount > 0 ? (
      <Spinner size="xs" className="text-primary" />
    ) : summary.errorCount > 0 && showToolErrorIcon ? (
      <AlertCircle
        data-testid="tool-step-error-icon"
        className="h-3.5 w-3.5 text-destructive shrink-0"
      />
    ) : null;

  if (!text && !leadingIcon) return null;

  return (
    <div>
      <Button unstyled
        type="button"
        aria-label={text}
        onClick={() => setOpen((o) => !o)}
        className="inline-flex items-center gap-1.5 py-1.5 text-left text-xs text-muted-foreground hover:text-foreground"
      >
        {leadingIcon}
        <span className="inline-flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5">
          {summary.buckets.map((bucket, index) => (
            <span key={bucket.key} className="inline-flex items-center gap-1">
              {index > 0 ? <span>{separator}</span> : null}
              <BucketIcon bucket={bucket.key} />
              <span>{renderBucket(t, bucket)}</span>
            </span>
          ))}
          {text.slice(bucketText.length) ? (
            <span>{text.slice(bucketText.length)}</span>
          ) : null}
        </span>
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0" />
        )}
      </Button>
      {open ? (
        // 垂直主干：border-l 一条线从 summary 下方贯通到最后一个 row。
        // ml-[7px] 对齐 summary 行 leading icon 中心（h-3.5 = 14px，center
        // 在 x=7）。pl-3 给 row 内容留出离线的横向距离，ToolStepRow ::before
        // 短横线从主干接到 row 起点，组成"├──"分支感。
        <div className="ml-[7px] mt-1 flex flex-col gap-0.5 border-l border-border/60 pl-3">
          {displayedSteps.map((s) => (
            <ToolStepRow key={s.toolCallId} step={s} />
          ))}
        </div>
      ) : null}
      {interactionSummaries.length > 0 ? (
        <div className="flex flex-col gap-0.5 pb-1 text-sm leading-6 text-foreground">
          {interactionSummaries.map((value, index) => (
            <div key={`${value}-${index}`} className="min-w-0 break-words">
              {t("messageList.toolReceipt.interactionReceived", {
                summary: value,
              })}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function BucketIcon({ bucket }: { bucket: ToolBucket }) {
  const Icon = BUCKET_ICON[bucket];
  return (
    <Icon
      aria-hidden="true"
      data-testid={`tool-bucket-icon-${bucket}`}
      className="h-3.5 w-3.5 shrink-0 text-muted-foreground/80"
    />
  );
}

function renderBucket(
  t: ReturnType<typeof useTranslation>["t"],
  bucket: BucketCount,
): string {
  return t(`toolGroupSummary.bucket.${bucket.key}`, { count: bucket.count });
}

function summarizeInteractionOutput(output?: string): string | null {
  if (!output) return null;
  const match = output.match(
    /^User has answered your questions:\s*([\s\S]*?)\.\s*You can now continue/,
  );
  if (!match) return null;
  const values = parseQuestionAnswerValues(match[1] ?? "")
    .map(compactReceiptText)
    .filter(Boolean);
  if (values.length === 0) return null;

  const visible = values.slice(0, 6);
  const suffix =
    values.length > visible.length ? ` +${values.length - visible.length}` : "";
  return `${visible.join(" / ")}${suffix}`;
}

function parseQuestionAnswerValues(text: string): string[] {
  const values: string[] = [];
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

    const quotedValue = parseQuotedString(text, i);
    if (quotedValue) {
      values.push(quotedValue.value.trim());
      i = quotedValue.next;
    } else {
      const start = i;
      while (i < text.length && text[i] !== ",") i += 1;
      values.push(text.slice(start, i).trim());
    }
  }

  return values.filter(Boolean);
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

function compactReceiptText(value: string): string {
  const normalized = value.replace(/\s+/g, " ").trim();
  if (normalized.length <= 42) return normalized;
  return `${normalized.slice(0, 41)}…`;
}
