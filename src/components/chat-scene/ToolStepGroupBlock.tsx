import { AlertCircle, ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ToolStepRow } from './ToolStepRow'
import { summarizeToolSteps, type BucketCount } from './toolStepSummary'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'

const MIN_RUNNING_DISPLAY_MS = 800

/**
 * 给每个 step 套"running 状态最小持续时间"——本地工具瞬间完成
 * (Read/Grep < 100ms) 会导致 loading 一闪而过看不清。后端真实切到 done/error
 * 时如果该 step 的 running 时长未满 minMs，UI 上延迟到 minMs 才切。
 *
 * 历史会话加载（一开始就是 done，没经过 running）不延迟，直接显示真实状态。
 */
function useDelayedSteps(steps: readonly RenderToolStep[], minMs: number): RenderToolStep[] {
  const startTimesRef = useRef<Map<string, number>>(new Map())
  const [pendingIds, setPendingIds] = useState<Set<string>>(new Set())

  useEffect(() => {
    const now = Date.now()
    const timers: ReturnType<typeof setTimeout>[] = []
    let nextPending = pendingIds
    let changed = false

    for (const step of steps) {
      const id = step.toolCallId
      if (step.status === 'running') {
        if (!startTimesRef.current.has(id)) startTimesRef.current.set(id, now)
        if (nextPending.has(id)) {
          if (!changed) {
            nextPending = new Set(nextPending)
            changed = true
          }
          nextPending.delete(id)
        }
        continue
      }
      const startedAt = startTimesRef.current.get(id)
      if (startedAt == null) continue // 从未 running（历史加载），不延迟
      const elapsed = now - startedAt
      if (elapsed >= minMs) {
        startTimesRef.current.delete(id)
        if (nextPending.has(id)) {
          if (!changed) {
            nextPending = new Set(nextPending)
            changed = true
          }
          nextPending.delete(id)
        }
        continue
      }
      // running 时长不足 minMs，需要延迟显示 done
      if (!nextPending.has(id)) {
        if (!changed) {
          nextPending = new Set(nextPending)
          changed = true
        }
        nextPending.add(id)
        timers.push(
          setTimeout(() => {
            startTimesRef.current.delete(id)
            setPendingIds((curr) => {
              if (!curr.has(id)) return curr
              const next = new Set(curr)
              next.delete(id)
              return next
            })
          }, minMs - elapsed),
        )
      }
    }

    if (changed) setPendingIds(nextPending)
    return () => timers.forEach(clearTimeout)
    // pendingIds 不进 dep——它由当前 effect 自己维护，进 dep 会无限循环。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [steps, minMs])

  return useMemo(() => {
    if (pendingIds.size === 0) return steps as RenderToolStep[]
    return steps.map((step) =>
      pendingIds.has(step.toolCallId) ? { ...step, status: 'running' as const } : step,
    )
  }, [steps, pendingIds])
}

interface ToolStepGroupBlockProps {
  steps: readonly RenderToolStep[]
}

/**
 * 一级折叠容器：把连续工具调用合并为一行 Codex 风格摘要
 * （"读取了 3 个文件、运行了 2 个命令 ›"）。点击展开后渲染 N 个 ToolStepRow，
 * 每行再各自可二级展开为输入/输出详情。无 border / 无 bg / 无 shadow。
 */
export function ToolStepGroupBlock({ steps }: ToolStepGroupBlockProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const displayedSteps = useDelayedSteps(steps, MIN_RUNNING_DISPLAY_MS)

  if (displayedSteps.length === 0) return null

  const summary = summarizeToolSteps(displayedSteps)
  const parts = summary.buckets.map((b) => renderBucket(t, b))
  const separator = t('toolGroupSummary.separator')
  let text = parts.join(separator)
  if (summary.runningCount > 0) {
    text += t('toolGroupSummary.runningSuffix')
  }
  if (summary.errorCount > 0) {
    text += separator + t('toolGroupSummary.failedSuffix', { count: summary.errorCount })
  }

  const leadingIcon =
    summary.runningCount > 0 ? (
      <Loader2 className="h-3.5 w-3.5 animate-spin text-primary shrink-0" />
    ) : summary.errorCount > 0 ? (
      <AlertCircle className="h-3.5 w-3.5 text-destructive shrink-0" />
    ) : null

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="inline-flex items-center gap-1.5 py-1.5 text-left text-sm text-muted-foreground hover:text-foreground"
      >
        {leadingIcon}
        <span>{text}</span>
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0" />
        )}
      </button>
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
    </div>
  )
}

function renderBucket(
  t: ReturnType<typeof useTranslation>['t'],
  bucket: BucketCount,
): string {
  return t(`toolGroupSummary.bucket.${bucket.key}`, { count: bucket.count })
}
