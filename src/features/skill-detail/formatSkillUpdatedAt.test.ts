import { describe, expect, it } from 'vitest'
import { formatSkillUpdatedAt } from './formatSkillUpdatedAt'

describe('formatSkillUpdatedAt', () => {
  it('把 RFC 3339 字符串格式化为 YYYY-MM-DD HH:MM:SS', () => {
    // 用本地时区 fix 一个具体时间：2026-05-13 10:30:00 本地
    const date = new Date(2026, 4, 13, 10, 30, 0)
    const iso = date.toISOString()
    expect(formatSkillUpdatedAt(iso)).toBe('2026-05-13 10:30:00')
  })

  it('补零到月/日/时/分/秒两位', () => {
    const date = new Date(2026, 0, 5, 7, 9, 4)
    const iso = date.toISOString()
    expect(formatSkillUpdatedAt(iso)).toBe('2026-01-05 07:09:04')
  })

  it('null / undefined / 空串返回 null', () => {
    expect(formatSkillUpdatedAt(null)).toBeNull()
    expect(formatSkillUpdatedAt(undefined)).toBeNull()
    expect(formatSkillUpdatedAt('')).toBeNull()
  })

  it('非法日期字符串返回 null', () => {
    expect(formatSkillUpdatedAt('not-a-date')).toBeNull()
  })
})
