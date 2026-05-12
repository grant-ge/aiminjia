import { describe, expect, it } from 'vitest'

import { describeFrequency } from './describeFrequency'

describe('describeFrequency', () => {
  it('one-shot returns formatted start time', () => {
    expect(
      describeFrequency(null, '2026-05-07T01:00:00Z', 'Asia/Shanghai'),
    ).toContain('2026')
  })

  it('daily interval=1', () => {
    expect(
      describeFrequency(
        { freq: 'daily', interval: 1, endCondition: { kind: 'never' } },
        '2026-05-07T01:00:00Z',
        'Asia/Shanghai',
      ),
    ).toContain('每天')
  })

  it('daily interval=2', () => {
    expect(
      describeFrequency(
        { freq: 'daily', interval: 2, endCondition: { kind: 'never' } },
        '2026-05-07T01:00:00Z',
        'Asia/Shanghai',
      ),
    ).toContain('每 2 天')
  })

  it('weekly with count', () => {
    expect(
      describeFrequency(
        { freq: 'weekly', interval: 1, endCondition: { kind: 'count', n: 5 } },
        '2026-05-07T01:00:00Z',
        'Asia/Shanghai',
      ),
    ).toContain('共 5 次')
  })
})
