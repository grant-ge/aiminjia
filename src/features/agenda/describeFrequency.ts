import type { Freq, RecurrenceRule } from '@/lib/tauri'

const FREQ_NOUN: Record<Freq, string> = {
  daily: '天',
  weekly: '周',
  monthly: '月',
  yearly: '年',
}

const FREQ_INTERVAL_ONE: Record<Freq, string> = {
  daily: '每天',
  weekly: '每周',
  monthly: '每月',
  yearly: '每年',
}

export function describeFrequency(
  rule: RecurrenceRule | null,
  startAt: string,
  timezone: string,
): string {
  const dt = new Date(startAt)

  if (!rule) {
    return formatDateTime(dt, timezone)
  }

  const intervalLabel =
    rule.interval === 1
      ? FREQ_INTERVAL_ONE[rule.freq]
      : `每 ${rule.interval} ${FREQ_NOUN[rule.freq]}`

  let endLabel = ''
  if (rule.endCondition.kind === 'count') {
    endLabel = `，共 ${rule.endCondition.n} 次`
  } else if (rule.endCondition.kind === 'until') {
    endLabel = `，至 ${formatDate(new Date(rule.endCondition.at), timezone)}`
  }

  return `${intervalLabel} ${formatTime(dt, timezone)}${endLabel}`
}

function formatDateTime(dt: Date, timezone: string): string {
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: timezone,
    hour12: false,
  }).format(dt)
}

function formatTime(dt: Date, timezone: string): string {
  return new Intl.DateTimeFormat('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    timeZone: timezone,
    hour12: false,
  }).format(dt)
}

function formatDate(dt: Date, timezone: string): string {
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    timeZone: timezone,
  }).format(dt)
}
