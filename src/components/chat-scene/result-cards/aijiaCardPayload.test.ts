import { describe, expect, it } from 'vitest'

import { parseAijiaCardPayload } from './aijiaCardPayload'

describe('parseAijiaCardPayload', () => {
  it('returns null for invalid JSON', () => {
    expect(parseAijiaCardPayload('{ nope')).toBeNull()
  })

  it('returns null for unknown card types', () => {
    expect(parseAijiaCardPayload('{"type":"other","id":"x"}')).toBeNull()
  })

  it('parses a skill_created payload with snapshot fields', () => {
    expect(parseAijiaCardPayload(JSON.stringify({
      type: 'skill_created',
      skillId: 'sales-followup',
      title: '销售跟进',
      description: '整理客户下一步动作',
    }))).toEqual({
      type: 'skill_created',
      skillId: 'sales-followup',
      title: '销售跟进',
      description: '整理客户下一步动作',
    })
  })

  it('parses a schedule_created payload with snapshot fields', () => {
    expect(parseAijiaCardPayload(JSON.stringify({
      type: 'schedule_created',
      scheduleId: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      frequencyLabel: '每天 09:00',
      nextFireAt: '2026-06-13T09:00:00+08:00',
    }))).toEqual({
      type: 'schedule_created',
      scheduleId: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      frequencyLabel: '每天 09:00',
      nextFireAt: '2026-06-13T09:00:00+08:00',
    })
  })
})
