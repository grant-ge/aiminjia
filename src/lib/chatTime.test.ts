import { describe, expect, it } from 'vitest'

import { formatChatTime, formatDayLabel, isSameDay } from './chatTime'

describe('chatTime', () => {
  const now = new Date('2026-05-22T14:00:00') // 周五

  it('今天显示纯时分', () => {
    expect(formatChatTime('2026-05-22T09:05:00', now)).toBe('09:05')
  })

  it('昨天加前缀', () => {
    expect(formatChatTime('2026-05-21T22:30:00', now)).toBe('昨天 22:30')
  })

  it('本周内显示周几', () => {
    // 周二
    expect(formatChatTime('2026-05-19T08:00:00', now)).toBe('周二 08:00')
  })

  it('同年内更早显示月日', () => {
    expect(formatChatTime('2026-03-15T11:11:00', now)).toBe('3月15日 11:11')
  })

  it('跨年显示完整年月日', () => {
    expect(formatChatTime('2024-12-31T23:59:00', now)).toBe('2024年12月31日 23:59')
  })

  it('day label 今/昨/周/月日', () => {
    expect(formatDayLabel('2026-05-22T00:00:00', now)).toBe('今天')
    expect(formatDayLabel('2026-05-21T00:00:00', now)).toBe('昨天')
    expect(formatDayLabel('2026-05-19T00:00:00', now)).toBe('周二')
    expect(formatDayLabel('2026-03-15T00:00:00', now)).toBe('3月15日 周日')
  })

  it('isSameDay 忽略时分秒', () => {
    expect(isSameDay(new Date('2026-05-22T00:00:00'), new Date('2026-05-22T23:59:59'))).toBe(true)
    expect(isSameDay(new Date('2026-05-22T23:59:59'), new Date('2026-05-23T00:00:01'))).toBe(false)
  })

  it('无效 ISO 返回空串', () => {
    expect(formatChatTime('not-a-date', now)).toBe('')
    expect(formatDayLabel('not-a-date', now)).toBe('')
  })
})
