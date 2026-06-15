export function buildCreateSkillFromConversationPrompt(): string {
  return `请总结当前对话内容，并把其中可复用的工作流程创建为一个技能。

要求：
1. 从当前对话提炼技能名称、适用场景、输入、执行步骤、输出格式和注意事项。
2. 按当前技能系统规范创建技能并刷新技能列表。
3. 创建成功后，用 aijia-card 返回 skill_created 结果，至少包含 skillId。

返回格式示例：
\`\`\`aijia-card
{
  "type": "skill_created",
  "skillId": "created-skill-id",
  "title": "技能名称",
  "description": "技能说明"
}
\`\`\``;
}

export function buildCreateScheduleFromConversationPrompt(): string {
  return `请总结当前对话内容，并创建一个定时任务。

要求：
1. 把当前对话提炼成定时任务标题、任务提示词、建议频率和开始时间。
2. 使用定时任务能力创建任务；如果对话没有明确时间，请选择保守默认值并在结果说明。
3. 创建成功后，用 aijia-card 返回 schedule_created 结果，至少包含 scheduleId。

返回格式示例：
\`\`\`aijia-card
{
  "type": "schedule_created",
  "scheduleId": "created-schedule-id",
  "title": "定时任务标题",
  "prompt": "任务提示词",
  "frequencyLabel": "每天 09:00",
  "nextFireAt": "2026-06-13T09:00:00+08:00"
}
\`\`\``;
}
