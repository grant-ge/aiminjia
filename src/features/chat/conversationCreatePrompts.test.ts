import { describe, expect, it } from 'vitest'

import {
  buildCreateScheduleFromConversationPrompt,
  buildCreateSkillFromConversationPrompt,
} from './conversationCreatePrompts'

describe('conversation create prompts', () => {
  it('builds a skill creation prompt that requires a skill_created aijia-card', () => {
    const prompt = buildCreateSkillFromConversationPrompt()

    expect(prompt).toContain('请总结当前对话内容')
    expect(prompt).toContain('创建为一个技能')
    expect(prompt).toContain('技能名称、适用场景、输入、执行步骤、输出格式和注意事项')
    expect(prompt).toContain('```aijia-card')
    expect(prompt).toContain('"type": "skill_created"')
    expect(prompt).toContain('"skillId"')
  })

  it('builds a scheduled-task creation prompt that requires a schedule_created aijia-card', () => {
    const prompt = buildCreateScheduleFromConversationPrompt()

    expect(prompt).toContain('请总结当前对话内容')
    expect(prompt).toContain('创建一个定时任务')
    expect(prompt).toContain('标题、任务提示词、建议频率和开始时间')
    expect(prompt).toContain('```aijia-card')
    expect(prompt).toContain('"type": "schedule_created"')
    expect(prompt).toContain('"scheduleId"')
  })
})
