import { describe, expect, it } from 'vitest'

import { readFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'

function avatarHash(teamId: string, safeName: string) {
  const file = resolve(process.cwd(), 'public', 'expert-avatars', teamId, `${safeName}.svg`)
  return createHash('sha256').update(readFileSync(file)).digest('hex')
}

describe('expert avatar assets', () => {
  it('uses one stable avatar for the same expert name across teams', () => {
    expect(avatarHash('operations', 'CFO')).toBe(avatarHash('strategy', 'CFO'))
    expect(avatarHash('strategy', 'CFO')).toBe(avatarHash('investment', 'CFO'))
    expect(avatarHash('operations', '数据分析师')).toBe(avatarHash('retrospective', '数据分析师'))
  })
})
