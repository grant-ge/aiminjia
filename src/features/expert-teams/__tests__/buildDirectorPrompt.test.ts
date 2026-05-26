import { describe, expect, it } from 'vitest'
import { EXPERT_TEAMS, type ExpertTeam } from '../teams'
import { buildDirectorPrompt } from '../buildDirectorPrompt'

describe('buildDirectorPrompt', () => {
  it.each(EXPERT_TEAMS.map((t) => [t.id, t]))('renders %s prompt', (_id, team) => {
    const prompt = buildDirectorPrompt(team, '示例议题：是否拓展东南亚市场')
    expect(prompt).toMatchSnapshot()
  })

  it('roundtable branch tells facilitator to recruit when user did not name experts', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'roundtable')!
    const prompt = buildDirectorPrompt(team, '团队五年后的工作形态会是怎样')
    expect(prompt).toMatch(/自行召集/)
    expect(prompt).toMatch(/告知用户名单/)
  })

  it('debate branch produces正方→反方→裁决 structure', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'debate')!
    const prompt = buildDirectorPrompt(team, '是否引入 AI 全员替换初级岗')
    expect(prompt).toMatch(/正方/)
    expect(prompt).toMatch(/反方/)
    expect(prompt).toMatch(/裁决|裁定/)
  })

  it('rounds branch lists每位专家发表一轮观点', () => {
    const team = EXPERT_TEAMS.find((t) => t.id === 'strategy')!
    const prompt = buildDirectorPrompt(team, '是否启动 B 轮融资')
    expect(prompt).toMatch(/每位专家.*一轮观点/)
    expect(prompt).toMatch(/TeamCreate/)
    expect(prompt).toMatch(/战略推演团/)
  })

  it('uses snapshot English director prompt when provided', () => {
    const team: ExpertTeam = {
      id: 'remote-strategy-simulation',
      name: 'Strategy Simulation Team',
      emoji: 'S',
      tagline: 'Remote strategy team',
      experts: [
        {
          name: 'Market Analyst',
          agentName: 'market-analyst',
          persona: 'Maps demand and competitive pressure',
          emoji: 'A',
        },
      ],
      examples: [],
      composerPlaceholder: '',
      facilitationStyle: 'rounds',
      snapshot: {
        teamId: 'remote-strategy-simulation',
        version: '1',
        facilitationStyle: 'rounds',
        displayI18n: {
          'en-US': { name: 'Strategy Simulation Team' },
          'zh-CN': { name: '战略推演团' },
        },
        experts: [],
        directorPromptI18n: {
          'en-US': {
            template: 'Facilitate {teamName} for {topic}\n\nRoster:\n{roster}',
          },
          'zh-CN': {
            template: '请主持「{teamName}」讨论：{topic}\n\n{roster}',
          },
        },
      },
    }

    const prompt = buildDirectorPrompt(team, 'market expansion', 'en-US')

    expect(prompt).toContain('Facilitate Strategy Simulation Team for market expansion')
  })
})
