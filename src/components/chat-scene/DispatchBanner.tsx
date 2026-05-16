/**
 * Centered system-banner rendering of a parsed dispatch prompt.
 *
 * Visual layout (chosen by user in 2026-05-15 design session):
 *
 *     ─────── 🛠 派活给小工 · 技术支持 · 今天 14:30 ───────
 *            群关键词：技术 / 对接 / 集成 · 排除：内部 / 测试
 *                     知识库 2 份 · 专业风格 · 每周汇总
 *
 * Default = fully expanded (no toggle). The banner replaces the right-aligned
 * user bubble in `UserMessageBubble` when `parseDispatchHeader` returns a
 * non-null result.
 */
import type { DispatchHeader } from './parseDispatchHeader'

interface DispatchBannerProps {
  header: DispatchHeader
}

function formatTriggerLabel(header: DispatchHeader): string {
  if (header.trigger === 'on-demand') return '按需派活'
  if (header.triggerTime) return `定时 ${header.triggerTime}`
  return '定时触发'
}

export function DispatchBanner({ header }: DispatchBannerProps) {
  const triggerLabel = formatTriggerLabel(header)
  return (
    <div data-testid="dispatch-banner" className="flex w-full flex-col items-center gap-1.5 py-1">
      <div className="flex w-full items-center gap-2 text-xs text-muted-foreground">
        <span className="h-px flex-1 bg-border" aria-hidden />
        <span className="flex items-center gap-1.5 whitespace-nowrap font-medium text-foreground">
          <span aria-hidden>🛠</span>
          <span>派活给 {header.employee}</span>
          {header.role ? (
            <>
              <span aria-hidden className="text-muted-foreground">·</span>
              <span>{header.role}</span>
            </>
          ) : null}
          <span aria-hidden className="text-muted-foreground">·</span>
          <span className="text-muted-foreground">{triggerLabel}</span>
        </span>
        <span className="h-px flex-1 bg-border" aria-hidden />
      </div>
      {header.configLines.length > 0 ? (
        <div className="flex w-full max-w-[80%] flex-col items-center gap-0.5 text-center text-xs text-muted-foreground">
          {header.configLines.map((line, i) => (
            <div key={i} className="leading-relaxed">
              {line}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  )
}
