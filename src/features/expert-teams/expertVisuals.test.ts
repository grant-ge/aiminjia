import { describe, expect, it } from 'vitest'
import { existsSync } from 'node:fs'
import { join } from 'node:path'

import {
  getExpertAvatarUrl,
  getExpertAvatarUrlForAgent,
  getExpertAvatarVisualForAgent,
  getRoundtablePlaceholderAvatarUrl,
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

  it('assigns stable placeholder avatars to open roundtable agents', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'roundtable')!

    expect(getExpertAvatarUrlForAgent(team, 'market-researcher')).toMatch(/^\/expert-avatars\/roundtable\/动态专家[一二三]\.svg$/)
    expect(getExpertAvatarVisualForAgent(team, 'market-researcher')).toEqual({
      kind: 'image',
      url: getRoundtablePlaceholderAvatarUrl('market-researcher'),
    })
    expect(getExpertAvatarVisualForAgent(team, 'market-researcher')).toEqual(getExpertAvatarVisualForAgent(team, 'market-researcher'))
  })

  it('maps remote stable agent names to display names and local HR atlas avatars', () => {
    expect(getExpertDisplayName(remoteHrTeam, 'compensation-expert')).toBe('薪酬专家')
    expect(getExpertAvatarVisualForAgent(remoteHrTeam, 'compensation-expert')).toEqual({
      kind: 'atlas',
      url: '/expert-avatars/hr-workplace/avatar-atlas.svg',
      x: 0,
      y: 96,
      w: 96,
      h: 96,
      atlasWidth: 384,
      atlasHeight: 288,
    })
  })

  it('prefers local HR atlas over remote expert team template atlases', () => {
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

    expect(getExpertAvatarVisualForAgent(team, 'compensation-expert')).toEqual({
      kind: 'atlas',
      url: '/expert-avatars/hr-workplace/avatar-atlas.svg',
      x: 0,
      y: 96,
      w: 96,
      h: 96,
      atlasWidth: 384,
      atlasHeight: 288,
    })
  })

  it('disambiguates remote hrbp experts by avatar name', () => {
    expect(getExpertAvatarVisualForAgent({
      ...remoteHrTeam,
      experts: [{
        name: '温嘉言',
        avatarName: '温嘉言',
        agentName: 'hrbp',
        avatar: '温',
        avatarText: '温',
        persona: '连接业务目标与团队实际情况，关注落地阻力',
        emoji: '🤝',
      }],
    }, 'hrbp')).toEqual({
      kind: 'atlas',
      url: '/expert-avatars/hr-workplace/avatar-atlas.svg',
      x: 192,
      y: 96,
      w: 96,
      h: 96,
      atlasWidth: 384,
      atlasHeight: 288,
    })

    expect(getExpertAvatarVisualForAgent({
      ...remoteHrTeam,
      experts: [{
        name: '何远策',
        avatarName: '何远策',
        agentName: 'hrbp',
        avatar: '何',
        avatarText: '何',
        persona: '关注业务目标、关键岗位和组织承接能力',
        emoji: '🤝',
      }],
    }, 'hrbp')).toEqual({
      kind: 'atlas',
      url: '/expert-avatars/hr-workplace/avatar-atlas.svg',
      x: 96,
      y: 192,
      w: 96,
      h: 96,
      atlasWidth: 384,
      atlasHeight: 288,
    })
  })

  it('prefers local static avatars over remote legacy atlases', () => {
    const remoteAtlas = {
      kind: 'atlas' as const,
      url: 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/v1/avatar-atlas.svg',
      x: 0,
      y: 288,
      w: 96,
      h: 96,
      atlasWidth: 672,
      atlasHeight: 384,
    }
    const team = EXPERT_TEAMS.find((t) => t.id === 'investment')!
    const remoteTeam: ExpertTeam = {
      ...team,
      experts: team.experts.map((expert) => (
        expert.agentName === 'cfo'
          ? { ...expert, avatar: remoteAtlas, avatarText: 'C' }
          : expert
      )),
    }

    expect(getExpertAvatarVisualForAgent(remoteTeam, 'cfo')).toEqual({
      kind: 'image',
      url: '/expert-avatars/investment/CFO.svg',
    })
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
