import { describe, expect, it } from 'vitest'
import { parseDispatchHeader } from './parseDispatchHeader'

const XIAOGONG_PROMPT = `你现在是「小工」（技术支持）。
负责回答技术问题。

[按需派活]

【本次工作配置】
- 群关键词：技术、对接、集成
- 排除关键词：内部
- 响应风格：专业

请立即开始按职责执行，不要等待用户额外指示。`

const XIAOYUAN_CRON_PROMPT = `你现在是「小研」（行业/竞品调研员）。
聚焦事实

[定时触发] 触发时间：2026-05-15 09:00 UTC

【本次工作配置】
- 默认技能：competitive-intelligence —— 请第一步调用 load_skill('competitive-intelligence') 加载工作流
- 监听目标（2 个）：Anthropic（https://anthropic.com）；OpenAI（https://openai.com）

请立即开始按职责执行，不要等待用户额外指示。`

describe('parseDispatchHeader', () => {
  it('returns null for plain user message', () => {
    expect(parseDispatchHeader('帮我查一下昨天的销售额')).toBeNull()
  })

  it('returns null for empty / null input', () => {
    expect(parseDispatchHeader('')).toBeNull()
    expect(parseDispatchHeader(null)).toBeNull()
    expect(parseDispatchHeader(undefined)).toBeNull()
  })

  it('parses on-demand xiaogong dispatch with config lines', () => {
    const h = parseDispatchHeader(XIAOGONG_PROMPT)
    expect(h).not.toBeNull()
    expect(h!.employee).toBe('小工')
    expect(h!.role).toBe('技术支持')
    expect(h!.trigger).toBe('on-demand')
    expect(h!.triggerTime).toBeNull()
    expect(h!.configLines).toEqual([
      '群关键词：技术、对接、集成',
      '排除关键词：内部',
      '响应风格：专业',
    ])
  })

  it('parses cron xiaoyuan dispatch with trigger time', () => {
    const h = parseDispatchHeader(XIAOYUAN_CRON_PROMPT)
    expect(h).not.toBeNull()
    expect(h!.employee).toBe('小研')
    expect(h!.role).toBe('行业/竞品调研员')
    expect(h!.trigger).toBe('cron')
    expect(h!.triggerTime).toBe('2026-05-15 09:00 UTC')
    expect(h!.configLines).toHaveLength(2)
    expect(h!.configLines[1]).toContain('监听目标')
  })

  it('parses dispatch without 【本次工作配置】 block (configLines empty)', () => {
    const minimal = `你现在是「小法」（合同审阅员）。
按 10 大风险条款扫描

[按需派活]

请立即开始按职责执行，不要等待用户额外指示。`
    const h = parseDispatchHeader(minimal)
    expect(h).not.toBeNull()
    expect(h!.employee).toBe('小法')
    expect(h!.configLines).toEqual([])
  })

  it('stops parsing config lines at "请立即" suffix', () => {
    const h = parseDispatchHeader(XIAOGONG_PROMPT)!
    // Should NOT include any line starting with 请立即
    expect(h.configLines.every((l) => !l.startsWith('请立即'))).toBe(true)
  })

  it('does not match a fake identity line elsewhere in the body', () => {
    const fake = `用户说：「你现在是「老板」（CEO）。」请帮我处理。`
    expect(parseDispatchHeader(fake)).toBeNull()
  })
})
