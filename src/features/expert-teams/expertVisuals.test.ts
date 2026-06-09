import { describe, expect, it } from 'vitest'
import { existsSync } from 'node:fs'
import { join } from 'node:path'

import {
  getExpertAvatarUrl,
  getExpertAvatarUrlForAgent,
  getExpertAvatarVisualForAgent,
} from './expertAvatar'
import { EXPERT_TEAMS, getExpertDisplayName, type ExpertTeam } from './teams'

const remoteHrTeam: ExpertTeam = {
  id: 'performance-compensation',
  name: '薪酬绩效评审团',
  emoji: '⚖️',
  tagline: '绩效校准 / 调薪方案 / 公平性复核',
  examples: [],
  composerPlaceholder: '告诉他们你要评审的绩效或薪酬方案...',
  facilitationStyle: 'rounds',
  experts: [
    {
      name: '薪酬专家',
      avatarName: '薪酬专家',
      agentName: 'compensation-expert',
      avatar: '薪',
      avatarText: '薪',
      persona: '关注薪酬结构、分位对标和内部公平性',
      emoji: '💰',
    },
  ],
}

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

  it('maps remote stable agent names to display names and avatar text', () => {
    expect(getExpertDisplayName(remoteHrTeam, 'compensation-expert')).toBe('薪酬专家')
    expect(getExpertAvatarVisualForAgent(remoteHrTeam, 'compensation-expert')).toEqual({
      kind: 'text',
      text: '薪',
    })
  })

  it('uses atlas avatars supplied by remote expert team templates', () => {
    const atlasAvatar = {
      kind: 'atlas' as const,
      url: 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/hr-v1/avatar-atlas.svg',
      x: 0,
      y: 96,
      w: 96,
      h: 96,
      atlasWidth: 384,
      atlasHeight: 288,
    }
    const team: ExpertTeam = {
      ...remoteHrTeam,
      experts: [{
        ...remoteHrTeam.experts[0],
        avatar: atlasAvatar,
        avatarText: '薪',
      }],
    }

    expect(getExpertAvatarVisualForAgent(team, 'compensation-expert')).toEqual(atlasAvatar)
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
