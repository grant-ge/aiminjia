import type { ExpertTeam } from './teams'

function renderRoster(team: ExpertTeam): string {
  if (team.experts.length === 0) return '（待主持人按议题召集）'
  return team.experts
    .map((e) => {
      const tag = e.agentName ? ` [name="${e.agentName}"]` : ''
      return `- ${e.emoji} ${e.name}${tag}：${e.persona}`
    })
    .join('\n')
}

/** Common spawn convention reminder shared by all facilitation styles. */
const SPAWN_NAME_RULE = `**spawn 子代理时，name 参数必须严格使用 \`[name="..."]\` 中给定的值（如 ceo、cfo、analyst），不要自创翻译或缩写**。这是前端识别每位专家的依据；不一致会导致头像、消息归属错乱。`

function renderSnapshotPrompt(
  team: ExpertTeam,
  topic: string,
  language?: string,
): string | null {
  const template =
    team.snapshot?.directorPromptI18n?.[language === 'en-US' ? 'en-US' : 'zh-CN']
      ?.template ?? team.snapshot?.directorPromptI18n?.['zh-CN']?.template
  if (!template) return null
  return template
    .replaceAll('{{teamName}}', team.name)
    .replaceAll('{{topic}}', topic)
    .replaceAll('{{roster}}', renderRoster(team))
    .replaceAll('{teamName}', team.name)
    .replaceAll('{topic}', topic)
    .replaceAll('{roster}', renderRoster(team))
}

function rounds(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」圆桌讨论。

# 团队成员
${renderRoster(team)}

# 用户提出的议题
${topic}

# 执行要求
1. 调用 TeamCreate 创建团队（team_name = "${team.name}"）
2. 为以上每位专家分别用 Agent 工具 spawn 子代理，把名字与 persona 注入到他们的 system prompt
3. ${SPAWN_NAME_RULE}
4. 让每位专家就议题发表一轮观点（每人 200-400 字），互相点评后给出共识 / 分歧
5. 你作为主持人（Lead）整理最终决策建议，呈现给用户`
}

function debate(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」结构化辩论。

# 团队成员
${renderRoster(team)}

# 用户提出的辩题
${topic}

# 执行要求
1. 调用 TeamCreate 创建团队（team_name = "${team.name}"）
2. 用 Agent 工具 spawn 正方、反方、观察员三个子代理，把 persona 注入 system prompt
3. ${SPAWN_NAME_RULE}
4. 按以下流程进行 2-3 轮：
   - 正方陈述 → 反方陈述
   - 反方质询正方 → 正方反驳
   - 观察员点评双方论点强弱
5. 你作为主持人（Lead）做最终裁决，给出建议立场与理由`
}

function openTable(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」开放圆桌讨论。

# 用户提出的议题
${topic}

# 执行要求
1. 你是主持人（Lead）。先判断议题需要哪些专业视角：
   - 若用户已在议题里点名了具体角色，优先采用
   - 否则你自行召集 3-5 位合适的专家
2. 在首条回复里**明确告知用户名单**（名字 + 一句话身份），让用户知晓召集到的专家
3. 调用 TeamCreate 创建团队（team_name = "${team.name}"）
4. 用 Agent 工具为每位专家 spawn 子代理，注入 name + persona
5. spawn 时给每位专家一个稳定的 name（小写英文 / kebab-case，如 ceo、growth-hacker），并保持后续轮次复用同一 name
6. 让每位专家就议题发言一轮（每人 200-400 字），你串场并最终汇总观点`
}

export function buildDirectorPrompt(
  team: ExpertTeam,
  userTopic: string,
  language?: string,
): string {
  const topic = userTopic.trim()
  const fromSnapshot = renderSnapshotPrompt(team, topic, language)
  if (fromSnapshot) return fromSnapshot

  switch (team.facilitationStyle) {
    case 'debate':
      return debate(team, topic)
    case 'open':
      return openTable(team, topic)
    case 'rounds':
    default:
      return rounds(team, topic)
  }
}
