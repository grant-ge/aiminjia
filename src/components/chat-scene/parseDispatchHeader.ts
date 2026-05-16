/**
 * Parse the dispatch-prompt header out of a user message.
 *
 * The backend's `build_dispatch_prompt` produces a body that begins with:
 *
 *     你现在是「<name>」（<role>）。
 *     <description (1 line)>
 *     <optional system_prompt_extra block>
 *
 *     [按需派活] | [定时触发 触发时间：YYYY-MM-DD HH:MM UTC]
 *     <optional user request>
 *
 *     【本次工作配置】
 *     - <line 1>
 *     - <line 2>
 *     ...
 *
 *     请立即开始按职责执行，不要等待用户额外指示。
 *
 * Returns null when `text` doesn't look like a dispatch prompt.
 *
 * This is a presentation-layer detector — it must not be load-bearing for
 * anything beyond UI rendering. Old messages without `【本次工作配置】`
 * blocks still parse (configLines = []); the banner just renders without a
 * config section.
 *
 * PR-9: the dispatch prompt mixes user-facing facts ("群关键词：技术")
 * with LLM-only instructions ("默认技能：x —— 请第一步调用 Skill(...)").
 * To keep the LLM behavior unchanged while showing users only the facts,
 * we lift two known structured fields out of the bullet list:
 *   - `skillId`           — from "- 默认技能：<id> ——"
 *   - `monitoringTargets` — from "- 监听目标（N 个）：name（url）；..."
 * The remaining bullets land in `configLines` and render as-is.
 */
export interface DispatchMonitoringTarget {
  name: string
  url: string | null
}

export interface DispatchHeader {
  /** Employee display name as configured (e.g. "小工"). */
  employee: string
  /** Employee role label (e.g. "技术支持"). */
  role: string
  /** "on-demand" for `[按需派活]`, "cron" for `[定时触发]`. */
  trigger: 'on-demand' | 'cron'
  /** Raw trigger-time string for cron dispatches; null for on-demand. */
  triggerTime: string | null
  /** Default skill id extracted from "- 默认技能：xxx" line; null if absent. */
  skillId: string | null
  /** Monitoring targets extracted from "- 监听目标（N 个）：..." line. */
  monitoringTargets: DispatchMonitoringTarget[]
  /**
   * Remaining bullet lines under 【本次工作配置】, with the leading "- "
   * stripped. Lines that have been lifted into `skillId` /
   * `monitoringTargets` are NOT present here.
   */
  configLines: string[]
}

const IDENTITY_RE = /^你现在是「(.+?)」（(.+?)）。/
const TRIGGER_RE = /\[(按需派活|定时触发)\](?:\s*触发时间：([^\n]+))?/
const SKILL_LINE_RE = /^默认技能：([^\s—]+)/
const MONITORING_LINE_RE = /^监听目标[^：]*：(.+)$/
// Each target is "name（url）" separated by "；" (or just "name" without url).
// Inner parens may be Chinese 「（…）」 or ascii "(…)".
const MONITORING_TARGET_RE = /^(.+?)(?:[（(]([^）)]+)[）)])?$/

function parseMonitoringLine(line: string): DispatchMonitoringTarget[] {
  const m = MONITORING_LINE_RE.exec(line)
  if (!m) return []
  // Backend joins with "；"; tolerate ";" too.
  return m[1]
    .split(/；|;/)
    .map((seg) => seg.trim())
    .filter((seg) => seg.length > 0)
    .map((seg) => {
      const tm = MONITORING_TARGET_RE.exec(seg)
      if (!tm) return { name: seg, url: null }
      return { name: tm[1].trim(), url: tm[2]?.trim() || null }
    })
}

export function parseDispatchHeader(text: string | null | undefined): DispatchHeader | null {
  if (!text) return null
  const idMatch = IDENTITY_RE.exec(text)
  if (!idMatch) return null
  const triggerMatch = TRIGGER_RE.exec(text)
  if (!triggerMatch) return null

  const trigger: DispatchHeader['trigger'] = triggerMatch[1] === '定时触发' ? 'cron' : 'on-demand'
  const triggerTime = triggerMatch[2]?.trim() || null

  // Pull the 【本次工作配置】 block; lines after it until a blank line or
  // the trailing "请立即..." suffix are config bullets.
  const rawLines: string[] = []
  const blockIdx = text.indexOf('【本次工作配置】')
  if (blockIdx >= 0) {
    const tail = text.slice(blockIdx + '【本次工作配置】'.length)
    const lines = tail.split('\n')
    for (const raw of lines) {
      const line = raw.trim()
      if (!line) {
        if (rawLines.length > 0) break
        continue
      }
      if (line.startsWith('请立即')) break
      if (line.startsWith('- ')) {
        rawLines.push(line.slice(2).trim())
      } else if (line.startsWith('-')) {
        rawLines.push(line.slice(1).trim())
      } else {
        // Unrecognized non-bullet line — stop parsing; this is likely the
        // trailing user_block or suffix.
        break
      }
    }
  }

  // Lift skill / monitoring lines out of the bullet list. The remaining
  // bullets render as plain text in the banner.
  let skillId: string | null = null
  let monitoringTargets: DispatchMonitoringTarget[] = []
  const configLines: string[] = []
  for (const line of rawLines) {
    const skillM = SKILL_LINE_RE.exec(line)
    if (skillM) {
      skillId = skillM[1].trim()
      continue
    }
    if (line.startsWith('监听目标')) {
      const parsed = parseMonitoringLine(line)
      if (parsed.length > 0) {
        monitoringTargets = parsed
        continue
      }
    }
    configLines.push(line)
  }

  return {
    employee: idMatch[1].trim(),
    role: idMatch[2].trim(),
    trigger,
    triggerTime,
    skillId,
    monitoringTargets,
    configLines,
  }
}
