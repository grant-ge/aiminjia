import { describe, expect, it } from 'vitest'
import { formatRelativeNextRun } from './timeFormat'

describe('formatRelativeNextRun', () => {
  // Anchor "now" so the tests are deterministic
  const now = new Date('2026-05-06T10:00:00+08:00').getTime()

  it('today + future hour', () => {
    expect(formatRelativeNextRun('2026-05-06T15:30:00+08:00', now)).toBe('今天 15:30')
  })

  it('tomorrow', () => {
    expect(formatRelativeNextRun('2026-05-07T09:00:00+08:00', now)).toBe('明天 09:00')
  })

  it('within this week', () => {
    // 2026-05-09 is a Saturday
    expect(formatRelativeNextRun('2026-05-09T08:30:00+08:00', now)).toBe('周六 08:30')
  })

  it('beyond this week falls back to date', () => {
    expect(formatRelativeNextRun('2026-05-20T09:00:00+08:00', now)).toBe('5月20日 09:00')
  })

  it('past returns empty string', () => {
    expect(formatRelativeNextRun('2026-05-06T09:00:00+08:00', now)).toBe('')
  })
})
