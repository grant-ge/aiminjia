/**
 * Centered system-banner rendering of a parsed dispatch prompt.
 *
 * Visual layout (PR-4 + PR-9 refinement):
 *
 *     ─────── 🛠 派活给 小工 · 技术支持 · 按需派活 ───────
 *
 *       群关键词：技术、对接、集成
 *       排除关键词：内部
 *       默认技能：[竞品调研]
 *       监听目标：[悟空] [workbuddy]
 *
 * Header is centered (divider lines + title); content area is **left-aligned**.
 * Skill ids are translated to displayName via `useSkillStore.getById`;
 * monitoring targets render as chips with the URL on title hover.
 */
import { useSkillStore } from '@/stores/skillStore'
import type { DispatchHeader, DispatchMonitoringTarget } from './parseDispatchHeader'

interface DispatchBannerProps {
  header: DispatchHeader
}

function formatTriggerLabel(header: DispatchHeader): string {
  if (header.trigger === 'on-demand') return '按需派活'
  if (header.triggerTime) return `定时 ${header.triggerTime}`
  return '定时触发'
}

function SkillChip({ id }: { id: string }) {
  const skill = useSkillStore((s) => s.getById(id))
  const label = skill?.displayName || id
  return (
    <span
      data-testid="dispatch-skill-chip"
      title={skill?.shortDescription || skill?.description || id}
      className="inline-flex items-center rounded-md bg-accent px-1.5 py-0.5 text-xs font-medium text-foreground"
    >
      {label}
    </span>
  )
}

function MonitoringTargetChip({ target }: { target: DispatchMonitoringTarget }) {
  return (
    <span
      data-testid="dispatch-monitoring-chip"
      title={target.url ?? target.name}
      className="inline-flex items-center rounded-md bg-accent px-1.5 py-0.5 text-xs font-medium text-foreground"
    >
      {target.name}
    </span>
  )
}

function ConfigRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 text-xs leading-relaxed">
      <span className="shrink-0 text-muted-foreground/70">{label}</span>
      <span className="flex flex-wrap items-center gap-1 text-foreground">{children}</span>
    </div>
  )
}

/**
 * Split "<label>：<rest>" into [label, rest]. Returns null if no colon (both
 * full-width "：" and ascii ":") is present.
 */
function splitLabel(line: string): [string, string] | null {
  const idx = (() => {
    const f = line.indexOf('：')
    const a = line.indexOf(':')
    if (f === -1) return a
    if (a === -1) return f
    return Math.min(f, a)
  })()
  if (idx <= 0) return null
  return [line.slice(0, idx), line.slice(idx + 1).trim()]
}

export function DispatchBanner({ header }: DispatchBannerProps) {
  const triggerLabel = formatTriggerLabel(header)
  const hasAnyConfig =
    header.configLines.length > 0 ||
    !!header.skillId ||
    header.monitoringTargets.length > 0

  return (
    <div data-testid="dispatch-banner" className="flex w-full flex-col gap-2 py-1">
      {/* Header — centered divider + title */}
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

      {/* Content — full-width within the chat column, label + value rows */}
      {hasAnyConfig ? (
        <div className="flex w-full flex-col gap-1 rounded-md bg-muted/30 px-3 py-2 text-left">
          {header.configLines.map((line, i) => {
            const split = splitLabel(line)
            if (split) {
              const [label, value] = split
              return (
                <ConfigRow key={`c-${i}`} label={label}>
                  <span>{value}</span>
                </ConfigRow>
              )
            }
            return (
              <div key={`c-${i}`} className="text-xs leading-relaxed text-foreground">
                {line}
              </div>
            )
          })}
          {header.skillId ? (
            <ConfigRow label="默认技能">
              <SkillChip id={header.skillId} />
            </ConfigRow>
          ) : null}
          {header.monitoringTargets.length > 0 ? (
            <ConfigRow label="监听目标">
              {header.monitoringTargets.map((t, i) => (
                <MonitoringTargetChip key={`m-${i}`} target={t} />
              ))}
            </ConfigRow>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
