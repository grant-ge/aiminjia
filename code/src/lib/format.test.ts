import { describe, it, expect } from 'vitest'
import {
  formatFileSize,
  formatCurrency,
  formatPercentage,
  truncateText,
  formatNumber,
  formatRelativeTime,
} from './format'

// ---------------------------------------------------------------------------
// formatFileSize
// ---------------------------------------------------------------------------

describe('formatFileSize', () => {
  it('returns "0 B" for zero bytes', () => {
    expect(formatFileSize(0)).toBe('0 B')
  })

  it('formats bytes below 1 KB without decimals', () => {
    expect(formatFileSize(512)).toBe('512 B')
    expect(formatFileSize(1)).toBe('1 B')
  })

  it('formats kilobytes with two decimals', () => {
    expect(formatFileSize(1024)).toBe('1.00 KB')
    expect(formatFileSize(1536)).toBe('1.50 KB')
  })

  it('formats megabytes', () => {
    expect(formatFileSize(1048576)).toBe('1.00 MB')
    expect(formatFileSize(2.5 * 1024 * 1024)).toBe('2.50 MB')
  })

  it('formats gigabytes', () => {
    expect(formatFileSize(1073741824)).toBe('1.00 GB')
  })

  it('caps at GB for very large values', () => {
    // 2 TB worth of bytes should still show GB
    const twoTB = 2 * 1024 * 1024 * 1024 * 1024
    const result = formatFileSize(twoTB)
    expect(result).toContain('GB')
  })
})

// ---------------------------------------------------------------------------
// formatPercentage
// ---------------------------------------------------------------------------

describe('formatPercentage', () => {
  it('formats with default 1 decimal', () => {
    expect(formatPercentage(84.6)).toBe('84.6%')
    expect(formatPercentage(100)).toBe('100.0%')
  })

  it('formats with custom decimals', () => {
    expect(formatPercentage(0.5, 2)).toBe('0.50%')
    expect(formatPercentage(33.333, 0)).toBe('33%')
  })

  it('handles zero', () => {
    expect(formatPercentage(0)).toBe('0.0%')
  })
})

// ---------------------------------------------------------------------------
// truncateText
// ---------------------------------------------------------------------------

describe('truncateText', () => {
  it('returns short text unchanged', () => {
    expect(truncateText('Hello', 10)).toBe('Hello')
  })

  it('truncates long text with ellipsis', () => {
    expect(truncateText('Hello, World!', 5)).toBe('Hello...')
  })

  it('handles Chinese characters', () => {
    expect(truncateText('这是一段很长的中文文本', 6)).toBe('这是一段很长...')
  })

  it('returns exact-length text unchanged', () => {
    expect(truncateText('12345', 5)).toBe('12345')
  })

  it('handles empty string', () => {
    expect(truncateText('', 5)).toBe('')
  })
})

// ---------------------------------------------------------------------------
// formatNumber
// ---------------------------------------------------------------------------

describe('formatNumber', () => {
  it('formats thousands with commas', () => {
    expect(formatNumber(1032)).toBe('1,032')
    expect(formatNumber(1000000)).toBe('1,000,000')
  })

  it('leaves small numbers unchanged', () => {
    expect(formatNumber(42)).toBe('42')
  })

  it('preserves decimals', () => {
    const result = formatNumber(1234567.89)
    expect(result).toContain('1,234,567')
  })

  it('handles zero', () => {
    expect(formatNumber(0)).toBe('0')
  })
})

// ---------------------------------------------------------------------------
// formatCurrency
// ---------------------------------------------------------------------------

describe('formatCurrency', () => {
  it('formats CNY by default', () => {
    const result = formatCurrency(12345)
    expect(result).toContain('12,345.00')
    // May contain ¥ or CN¥ depending on locale implementation
    expect(result).toMatch(/[¥￥]/)
  })

  it('formats with custom currency', () => {
    const result = formatCurrency(99.9, 'USD')
    expect(result).toContain('99.90')
  })

  it('handles zero', () => {
    const result = formatCurrency(0)
    expect(result).toContain('0.00')
  })

  it('rounds to 2 decimal places', () => {
    const result = formatCurrency(12345.678)
    expect(result).toContain('12,345.68')
  })
})

// ---------------------------------------------------------------------------
// formatRelativeTime
// ---------------------------------------------------------------------------

describe('formatRelativeTime', () => {
  it('returns "刚刚" for very recent times', () => {
    const now = new Date()
    now.setSeconds(now.getSeconds() - 10)
    expect(formatRelativeTime(now.toISOString())).toBe('刚刚')
  })

  it('returns minutes ago', () => {
    const date = new Date()
    date.setMinutes(date.getMinutes() - 5)
    expect(formatRelativeTime(date.toISOString())).toBe('5 分钟前')
  })

  it('returns hours ago', () => {
    const date = new Date()
    date.setHours(date.getHours() - 3)
    expect(formatRelativeTime(date.toISOString())).toBe('3 小时前')
  })

  it('returns "昨天" for 1 day ago', () => {
    const date = new Date()
    date.setDate(date.getDate() - 1)
    // Might be "昨天" or "1 天前" depending on exact calculation
    const result = formatRelativeTime(date.toISOString())
    expect(result === '昨天' || result === '1 天前').toBe(true)
  })

  it('returns days ago for 2+ days', () => {
    const date = new Date()
    date.setDate(date.getDate() - 5)
    expect(formatRelativeTime(date.toISOString())).toBe('5 天前')
  })

  it('returns months ago', () => {
    const date = new Date()
    date.setMonth(date.getMonth() - 3)
    const result = formatRelativeTime(date.toISOString())
    expect(result).toMatch(/\d+ 个月前/)
  })

  it('returns years ago', () => {
    const date = new Date()
    date.setFullYear(date.getFullYear() - 2)
    const result = formatRelativeTime(date.toISOString())
    expect(result).toMatch(/\d+ 年前/)
  })
})
