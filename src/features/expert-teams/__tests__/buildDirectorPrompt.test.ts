import { describe, expect, it } from 'vitest'
import { EXPERT_TEAMS, getExpertTeam } from '../teams'
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

  it('keeps Chinese director prompt data localized when given an English team object', () => {
    const team = getExpertTeam('strategy', 'en-US')!
    const prompt = buildDirectorPrompt(team, '是否拓展东南亚市场', 'zh-CN')

    expect(prompt).toContain('你现在的任务是为用户主持一场「战略推演团」圆桌讨论。')
    expect(prompt).toContain('必须全程使用中文回复用户')
    expect(prompt).toContain('team_name = "expert-team-strategy"')
    expect(prompt).not.toContain('Strategy Simulation Team')
  })

  it('keeps Business Decision Team out of Chinese operations prompts', () => {
    const team = getExpertTeam('operations', 'en-US')!
    const prompt = buildDirectorPrompt(team, 'Q2指标为何下滑', 'zh-CN')

    expect(prompt).toContain('「经营决策团」圆桌讨论')
    expect(prompt).toContain('必须全程使用中文回复用户')
    expect(prompt).not.toContain('Business Decision Team')
  })

  it('renders English director prompts consistently for English locale', () => {
    const team = getExpertTeam('strategy', 'en-US')!
    const prompt = buildDirectorPrompt(team, 'Should we expand into Southeast Asia?', 'en-US')

    expect(prompt).toContain('Your task is to host a "Strategy Simulation Team" roundtable discussion for the user.')
    expect(prompt).toContain('Team members')
    expect(prompt).toContain('Reply to the user in English throughout')
    expect(prompt).toContain('team_name = "expert-team-strategy"')
    expect(prompt).not.toContain('你现在的任务')
  })
})
