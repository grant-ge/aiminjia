/**
 * 聊天时间显示工具
 *
 * 混合策略：
 *   今天   → "14:32"
 *   昨天   → "昨天 14:32"
 *   本周   → "周三 14:32"
 *   更久   → "5月15日 14:32" / "2024年5月15日 14:32"
 *
 * hover 时通过 `formatFullDateTime` 给完整 ISO/本地化串。
 */

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate())
}

function daysBetween(a: Date, b: Date): number {
  const ms = startOfDay(b).getTime() - startOfDay(a).getTime()
  return Math.round(ms / 86400000)
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : String(n)
}

function formatHm(d: Date): string {
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`
}

const WEEKDAYS_ZH = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  )
}

/**
 * 气泡旁短时间戳。
 */
export function formatChatTime(iso: string, now: Date = new Date()): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return ''
  const diffDays = daysBetween(d, now)
  if (diffDays === 0) return formatHm(d)
  if (diffDays === 1) return `昨天 ${formatHm(d)}`
  if (diffDays > 0 && diffDays < 7) return `${WEEKDAYS_ZH[d.getDay()]} ${formatHm(d)}`
  const sameYear = d.getFullYear() === now.getFullYear()
  if (sameYear) return `${d.getMonth() + 1}月${d.getDate()}日 ${formatHm(d)}`
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 ${formatHm(d)}`
}

/**
 * 日分隔条文案：今天 / 昨天 / 周三 / 5月15日 / 2024年5月15日（带星期）。
 */
export function formatDayLabel(iso: string, now: Date = new Date()): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return ''
  const diffDays = daysBetween(d, now)
  if (diffDays === 0) return '今天'
  if (diffDays === 1) return '昨天'
  if (diffDays > 0 && diffDays < 7) return WEEKDAYS_ZH[d.getDay()]
  const sameYear = d.getFullYear() === now.getFullYear()
  const weekday = WEEKDAYS_ZH[d.getDay()]
  if (sameYear) return `${d.getMonth() + 1}月${d.getDate()}日 ${weekday}`
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 ${weekday}`
}

/**
 * 完整时间，用于 hover title 提示。
 */
export function formatFullDateTime(iso: string): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}
