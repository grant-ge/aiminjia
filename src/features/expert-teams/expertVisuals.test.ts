import { describe, expect, it } from 'vitest'
import { existsSync } from 'node:fs'
import { join } from 'node:path'

import { getExpertAvatarUrl, getExpertAvatarUrlForAgent } from './expertAvatar'
import { EXPERT_TEAMS } from './teams'

describe('expert visuals', () => {
  it('maps runtime teammate names to the matching expert avatar without changing display names', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'marketing')!

    expect(getExpertAvatarUrlForAgent(team, 'brand-lead')).toBe('/expert-avatars/marketing/品牌负责人.svg')
    expect(getExpertAvatarUrlForAgent(team, 'content-lead')).toBe('/expert-avatars/marketing/内容主理人.svg')
    expect(getExpertAvatarUrlForAgent(team, 'growth-hacker')).toBe('/expert-avatars/marketing/增长黑客.svg')
    expect(getExpertAvatarUrlForAgent(team, 'channel-manager')).toBe('/expert-avatars/marketing/渠道经理.svg')
  })

  it('falls back to null for unknown dynamic agent names', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'marketing')!

    expect(getExpertAvatarUrlForAgent(team, 'unknown-agent')).toBeNull()
  })

  it('has a committed avatar asset for every fixed-roster expert', () => {
    for (const team of EXPERT_TEAMS.filter((t) => t.experts.length > 0)) {
      for (const expert of team.experts) {
        const url = getExpertAvatarUrl(team.id, expert.name)
        expect(url, `${team.id}:${expert.name}`).toBeTruthy()
        expect(existsSync(join(process.cwd(), 'public', url!))).toBe(true)
      }
    }
  })

  it('maps every fixed runtime agentName to its expert avatar', () => {
    for (const team of EXPERT_TEAMS.filter((t) => t.experts.length > 0)) {
      for (const expert of team.experts) {
        expect(getExpertAvatarUrlForAgent(team, expert.agentName ?? expert.name), `${team.id}:${expert.agentName}`).toBe(
          getExpertAvatarUrl(team.id, expert.name),
        )
      }
    }
  })
})
