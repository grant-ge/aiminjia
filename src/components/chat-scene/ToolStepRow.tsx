import { AlertCircle, CheckCircle2, ChevronDown, ChevronRight } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import { ToolTraceIO } from './ToolTraceIO'
import type { RenderToolStep } from '@/hooks/useTurnRenderModel'
import { useDevSettingsStore } from '@/stores/devSettingsStore'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

interface ToolStepRowProps {
  step: RenderToolStep
}

/**
 * 单条工具行：左侧状态 icon + tool 名 + 主参数摘要 + 右侧 chevron。
 * 点击 toggle 二级展开，展开后复用 `ToolTraceIO` 显示输入/输出/progress。
 * Auto-expand: running 且有 progressTail 时自动展开，方便跟踪长跑命令输出。
 */
export function ToolStepRow({ step }: ToolStepRowProps) {
  const showToolErrorIcon = useDevSettingsStore((s) => s.showToolErrorIcon)
  const autoExpand =
    step.status === 'running' && (step.progressTail ?? '').length > 0
  const [manualOpen, setManualOpen] = useState<boolean | null>(null)
  const open = manualOpen ?? autoExpand

  // -translate-y-px：在 items-center 布局下 text-xs 文字偏低，icon 跟文字中线
  // 对齐而非跟连线（::before top-3）对齐——上移 1px 让 icon 中心刚好压在
  // 水平 stub 上，视觉上"连线穿过 icon 中心"。
  const statusIcon: ReactNode =
    step.status === 'running' ? (
      <Spinner size="xs" className="-translate-y-px text-primary" />
    ) : step.status === 'error' && showToolErrorIcon ? (
      <AlertCircle data-testid="tool-step-row-error-icon" className="h-3 w-3 -translate-y-px text-destructive" />
    ) : step.status === 'error' ? (
      null
    ) : (
      <CheckCircle2 className="h-3 w-3 -translate-y-px text-muted-foreground" />
    )

  const summary = formatStepSummary(step)

  return (
    // ::before 画一条短横线，从父容器的左侧 border-l 主干接到本 row 起点，
    // 视觉上呈现"├──"分支感。横线位置：top-3 ≈ py-1 row 内容垂直中线；
    // left-[-12px] 跨过父 pl-3 距离接到主干。
    //
    // `last:after`：最后一行用 bg-background 盖掉 stub 下面那段父级 border-l
    // 的延伸（border-l 是从父容器顶到底贯通的，最后一行 stub 在 row 中段，
    // stub 下面还会延伸 ~12px 到容器底），这样最后一行视觉上自然收成"└"。
    <div className="relative before:absolute before:left-[-12px] before:top-3 before:h-px before:w-3 before:bg-[rgba(var(--border-rgb),0.60)] last:after:absolute last:after:left-[-13px] last:after:top-3 last:after:bottom-0 last:after:w-px last:after:bg-background last:after:content-['']">
      <Button unstyled
        type="button"
        onClick={() => setManualOpen(open ? false : true)}
        className="inline-flex max-w-full items-center gap-1.5 py-1 text-left text-xs text-muted-foreground hover:text-foreground"
      >
        {statusIcon}
        <span className="truncate font-mono">{summary}</span>
        {open ? (
          <ChevronDown className="h-3 w-3 shrink-0" />
        ) : (
          <ChevronRight className="h-3 w-3 shrink-0" />
        )}
      </Button>
      {open ? (
        <div className="mt-1">
          <ToolTraceIO
            toolName={step.name}
            inputJson={step.inputJson}
            output={step.output}
            progressTail={step.status === 'running' ? step.progressTail : undefined}
            progressTotalBytes={
              step.status === 'running' ? step.progressTotalBytes : undefined
            }
          />
        </div>
      ) : null}
    </div>
  )
}

function formatStepSummary(step: RenderToolStep): string {
  const detail = extractDetail(step.name, step.inputJson)
  return detail ? `${step.name} ${detail}` : step.name
}

function extractDetail(name: string, inputJson?: string): string | null {
  if (!inputJson) return null
  let parsed: Record<string, unknown>
  try {
    parsed = JSON.parse(inputJson) as Record<string, unknown>
  } catch {
    return null
  }
  const lower = name.toLowerCase()

  if (lower === 'bash' || lower === 'shell' || lower === 'shell_run') {
    const cmd = pickString(parsed, ['command', 'cmd', 'script'])
    return cmd ? truncate(cmd, 80) : null
  }
  if (lower === 'read' || lower === 'read_file') {
    const p = pickString(parsed, ['file_path', 'path', 'filepath'])
    return p ? basename(p) : null
  }
  if (
    lower === 'write' ||
    lower === 'edit' ||
    lower === 'multiedit' ||
    lower === 'write_file' ||
    lower === 'edit_file'
  ) {
    const p = pickString(parsed, ['file_path', 'path', 'filepath'])
    return p ? basename(p) : null
  }
  if (lower === 'grep') {
    return pickString(parsed, ['pattern', 'query']) ?? null
  }
  if (lower === 'glob') {
    return pickString(parsed, ['pattern', 'glob']) ?? null
  }
  return null
}

function pickString(obj: Record<string, unknown>, keys: string[]): string | null {
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.length > 0) return v
  }
  return null
}

function basename(p: string): string {
  const m = p.replace(/\\/g, '/').split('/').filter(Boolean)
  return m.length === 0 ? p : m[m.length - 1]!
}

function truncate(s: string, n: number): string {
  return s.length <= n ? s : s.slice(0, n - 1) + '…'
}
