/**
 * Format helpers for the team chat drawer. Pure functions, no React imports.
 */

const TIME_FORMATTER = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
})

const DATE_TIME_FORMATTER = new Intl.DateTimeFormat('zh-CN', {
  month: 'numeric',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
})

/** "11:24:51" */
export function formatClock(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  return TIME_FORMATTER.format(d)
}

/** "5/13 11:24" */
export function formatShortDateTime(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  return DATE_TIME_FORMATTER.format(d)
}

/** "11:24 – 11:38" or "11:24 – 进行中" */
export function formatDuration(start: string, end: string | null): string {
  const s = formatClock(start)
  if (!end) return `${s} – 进行中`
  return `${s} – ${formatClock(end)}`
}

/** Render `5/13 14:30` only if the date differs from `previous`. */
export function formatTimestampForGroup(iso: string, previousIso: string | null): string | null {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return null
  if (!previousIso) return formatShortDateTime(iso)
  const prev = new Date(previousIso)
  if (Number.isNaN(prev.getTime())) return formatShortDateTime(iso)
  const SAME_GROUP_MS = 5 * 60 * 1000
  if (d.getTime() - prev.getTime() < SAME_GROUP_MS) return null
  return formatShortDateTime(iso)
}
