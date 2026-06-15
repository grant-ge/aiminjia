export interface SkillCreatedCardPayload {
  type: 'skill_created'
  skillId: string
  title?: string
  description?: string
}

export interface ScheduleCreatedCardPayload {
  type: 'schedule_created'
  scheduleId: string
  title?: string
  prompt?: string
  frequencyLabel?: string
  nextFireAt?: string
}

export type AijiaCardPayload = SkillCreatedCardPayload | ScheduleCreatedCardPayload

function optionalString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined
}

export function parseAijiaCardPayload(raw: string): AijiaCardPayload | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return null
  }

  if (!parsed || typeof parsed !== 'object') return null
  const record = parsed as Record<string, unknown>

  if (record.type === 'skill_created') {
    const skillId = optionalString(record.skillId)
    if (!skillId) return null
    return {
      type: 'skill_created',
      skillId,
      title: optionalString(record.title),
      description: optionalString(record.description),
    }
  }

  if (record.type === 'schedule_created') {
    const scheduleId = optionalString(record.scheduleId)
    if (!scheduleId) return null
    return {
      type: 'schedule_created',
      scheduleId,
      title: optionalString(record.title),
      prompt: optionalString(record.prompt),
      frequencyLabel: optionalString(record.frequencyLabel),
      nextFireAt: optionalString(record.nextFireAt),
    }
  }

  return null
}
