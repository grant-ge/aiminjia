import { describe, expect, it } from 'vitest'

import { getExpertAvatarStyleForAgent, getExpertAvatarUrlForAgent } from './expertAvatar'
import { EXPERT_TEAMS, type ExpertTeam } from './teams'

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

  it('maps OSS atlas avatars to CSS background styles without per-expert image URLs', () => {
    const team: ExpertTeam = {
      id: 'remote-marketing-council',
      name: '营销策划团',
      emoji: 'M',
      tagline: '',
      examples: [],
      composerPlaceholder: '',
      facilitationStyle: 'rounds',
      experts: [
        {
          name: '品牌负责人',
          agentName: 'brand-lead',
          persona: '关注定位、调性、长期心智占领',
          emoji: '🎨',
          avatar: {
            kind: 'atlas',
            url: 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/v1/avatar-atlas.svg',
            x: 96,
            y: 0,
            w: 96,
            h: 96,
            atlasWidth: 672,
            atlasHeight: 384,
          },
        },
      ],
    }

    expect(getExpertAvatarUrlForAgent(team, 'brand-lead')).toBeNull()
    expect(getExpertAvatarStyleForAgent(team, 'brand-lead')).toEqual({
      backgroundImage: 'url("https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/v1/avatar-atlas.svg")',
      backgroundPosition: '16.666666666666664% 0%',
      backgroundSize: '700% 400%',
    })
  })
})
