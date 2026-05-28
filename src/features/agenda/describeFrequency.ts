import type { TFunction } from 'i18next'

import type { Freq, RecurrenceRule } from '@/lib/tauri'

export function describeFrequency(
  rule: RecurrenceRule | null,
  startAt: string,
  timezone: string,
  t: TFunction,
  locale: string = 'zh-CN',
): string {
  const dt = new Date(startAt)

  if (!rule) {
    return formatDateTime(dt, timezone, locale)
  }

  const intervalLabel =
    rule.interval === 1
      ? t(`schedules.frequency.intervalOne.${rule.freq satisfies Freq}`)
      : t('schedules.frequency.intervalN', {
          n: rule.interval,
          unit: t(`schedules.frequency.noun.${rule.freq satisfies Freq}`),
        })

  let endLabel = ''
  if (rule.endCondition.kind === 'count') {
    endLabel = t('schedules.frequency.endCount', { n: rule.endCondition.n })
  } else if (rule.endCondition.kind === 'until') {
    endLabel = t('schedules.frequency.endUntil', {
      date: formatDate(new Date(rule.endCondition.at), timezone, locale),
    })
  }

  return (
    t('schedules.frequency.withTime', {
      interval: intervalLabel,
      time: formatTime(dt, timezone, locale),
    }) + endLabel
  )
}

function formatDateTime(dt: Date, timezone: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    timeZone: timezone,
    hour12: false,
  }).format(dt)
}

function formatTime(dt: Date, timezone: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
    timeZone: timezone,
    hour12: false,
  }).format(dt)
}

function formatDate(dt: Date, timezone: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    timeZone: timezone,
  }).format(dt)
}
