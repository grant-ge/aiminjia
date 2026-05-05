const WEEKDAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'] as const

const TZ = 'Asia/Shanghai'

/** Extract Y/M/D/h/m/dow in Asia/Shanghai for a given Date instance. */
function partsInTz(d: Date): { y: number; m: number; day: number; h: number; min: number; dow: number } {
  // en-CA gives YYYY-MM-DD; using parts gives stable numeric tokens.
  const fmt = new Intl.DateTimeFormat('en-US', {
    timeZone: TZ,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    weekday: 'short',
    hour12: false,
  })
  const parts = fmt.formatToParts(d)
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? ''
  const weekdayMap: Record<string, number> = {
    Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6,
  }
  // Intl returns "24" for midnight in some locales/runtimes; normalize.
  let h = parseInt(get('hour'), 10)
  if (h === 24) h = 0
  return {
    y: parseInt(get('year'), 10),
    m: parseInt(get('month'), 10),
    day: parseInt(get('day'), 10),
    h,
    min: parseInt(get('minute'), 10),
    dow: weekdayMap[get('weekday')] ?? 0,
  }
}

/** Day index since epoch in Asia/Shanghai (calendar day, ignoring time). */
function tzDayIndex(d: Date): number {
  const p = partsInTz(d)
  // Treat as a UTC midnight to get a stable day index. Different days in
  // Asia/Shanghai will differ by exactly 1.
  return Math.floor(Date.UTC(p.y, p.m - 1, p.day) / 86_400_000)
}

/**
 * Format an ISO timestamp as a friendly Chinese relative time, anchored to the
 * Asia/Shanghai calendar day so output is consistent across user machines:
 * - 今天 HH:mm   (within the current calendar day)
 * - 明天 HH:mm   (next calendar day)
 * - 周三 HH:mm   (within the next 6 days)
 * - 5月20日 HH:mm (anything ≥ 7 days out)
 *
 * Returns '' when the timestamp is in the past, so callers can render or hide
 * conditionally.
 *
 * `nowMs` is injectable for deterministic tests.
 */
export function formatRelativeNextRun(iso: string, nowMs: number = Date.now()): string {
  const d = new Date(iso)
  const ms = d.getTime()
  if (ms <= nowMs) return ''

  const now = new Date(nowMs)
  const dayDiff = tzDayIndex(d) - tzDayIndex(now)
  const p = partsInTz(d)
  const time = `${String(p.h).padStart(2, '0')}:${String(p.min).padStart(2, '0')}`

  if (dayDiff === 0) return `今天 ${time}`
  if (dayDiff === 1) return `明天 ${time}`
  if (dayDiff < 7) return `${WEEKDAYS[p.dow]} ${time}`
  return `${p.m}月${p.day}日 ${time}`
}
