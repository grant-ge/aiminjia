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
 */
export interface DispatchHeader {
  /** Employee display name as configured (e.g. "小工"). */
  employee: string
  /** Employee role label (e.g. "技术支持"). */
  role: string
  /** "on-demand" for `[按需派活]`, "cron" for `[定时触发]`. */
  trigger: 'on-demand' | 'cron'
  /** Raw trigger-time string for cron dispatches; null for on-demand. */
  triggerTime: string | null
  /** Bullet lines under 【本次工作配置】, with the leading "- " stripped. */
  configLines: string[]
}

const IDENTITY_RE = /^你现在是「(.+?)」（(.+?)）。/
const TRIGGER_RE = /\[(按需派活|定时触发)\](?:\s*触发时间：([^\n]+))?/

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
  const configLines: string[] = []
  const blockIdx = text.indexOf('【本次工作配置】')
  if (blockIdx >= 0) {
    const tail = text.slice(blockIdx + '【本次工作配置】'.length)
    const lines = tail.split('\n')
    for (const raw of lines) {
      const line = raw.trim()
      if (!line) {
        if (configLines.length > 0) break
        continue
      }
      if (line.startsWith('请立即')) break
      if (line.startsWith('- ')) {
        configLines.push(line.slice(2).trim())
      } else if (line.startsWith('-')) {
        configLines.push(line.slice(1).trim())
      } else {
        // Unrecognized non-bullet line — stop parsing; this is likely the
        // trailing user_block or suffix.
        break
      }
    }
  }

  return {
    employee: idMatch[1].trim(),
    role: idMatch[2].trim(),
    trigger,
    triggerTime,
    configLines,
  }
}
