import { getExpertTeam, type ExpertTeam } from './teams'

type PromptLocale = 'zh-CN' | 'en-US'

function normalizePromptLocale(language?: string): PromptLocale {
  return language?.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

function runtimeTeamName(team: ExpertTeam): string {
  return `expert-team-${team.id}`
}

function teamForPrompt(team: ExpertTeam, language?: string): ExpertTeam {
  if (team.directorPromptTemplate) return team
  return getExpertTeam(team.id, normalizePromptLocale(language)) ?? team
}

function renderRoster(team: ExpertTeam, locale: PromptLocale): string {
  if (team.experts.length === 0) {
    return locale === 'en-US'
      ? '(The director will invite experts based on the topic.)'
      : '（待主持人按议题召集）'
  }
  return team.experts
    .map((e) => {
      const tag = e.agentName ? ` [name="${e.agentName}"]` : ''
      return `- ${e.emoji} ${e.name}${tag}：${e.persona}`
    })
    .join('\n')
}

function renderDirectorPromptTemplate(
  template: string,
  team: ExpertTeam,
  topic: string,
  locale: PromptLocale,
): string {
  const values: Record<string, string> = {
    teamName: team.name,
    runtimeTeamName: runtimeTeamName(team),
    roster: renderRoster(team, locale),
    topic,
    spawnNameRule: spawnNameRule(locale),
  }
  return Object.entries(values).reduce(
    (out, [key, value]) => out.split(`{{${key}}}`).join(value),
    template,
  )
}

/** Common spawn convention reminder shared by all facilitation styles. */
function spawnNameRule(locale: PromptLocale): string {
  return locale === 'en-US'
    ? '**When spawning sub-agents, the name parameter must strictly use the value shown in `[name="..."]` (for example ceo, cfo, analyst). Do not invent translations or abbreviations.** The frontend uses this stable name to identify each expert; mismatches cause avatar and message attribution errors.'
    : '**spawn 子代理时，name 参数必须严格使用 `[name="..."]` 中给定的值（如 ceo、cfo、analyst），不要自创翻译或缩写**。这是前端识别每位专家的依据；不一致会导致头像、消息归属错乱。'
}

function roundsZh(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」圆桌讨论。

# 团队成员
${renderRoster(team, 'zh-CN')}

# 用户提出的议题
${topic}

# 执行要求
1. 调用 TeamCreate 创建团队（team_name = "${runtimeTeamName(team)}"，这是工具内部稳定名称；对用户展示仍使用「${team.name}」）
2. 为以上每位专家分别用 Agent 工具 spawn 子代理，把名字与 persona 注入到他们的 system prompt
3. ${spawnNameRule('zh-CN')}
4. 必须全程使用中文回复用户；团队名称、阶段说明、总结句都使用中文，不要夹带英文团队名
5. 让每位专家就议题发表一轮观点（每人 200-400 字），互相点评后给出共识 / 分歧
6. 你作为主持人（Lead）整理最终决策建议，呈现给用户`
}

function debateZh(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」结构化辩论。

# 团队成员
${renderRoster(team, 'zh-CN')}

# 用户提出的辩题
${topic}

# 执行要求
1. 调用 TeamCreate 创建团队（team_name = "${runtimeTeamName(team)}"，这是工具内部稳定名称；对用户展示仍使用「${team.name}」）
2. 用 Agent 工具 spawn 正方、反方、观察员三个子代理，把 persona 注入 system prompt
3. ${spawnNameRule('zh-CN')}
4. 必须全程使用中文回复用户；团队名称、阶段说明、总结句都使用中文，不要夹带英文团队名
5. 按以下流程进行 2-3 轮：
   - 正方陈述 → 反方陈述
   - 反方质询正方 → 正方反驳
   - 观察员点评双方论点强弱
6. 你作为主持人（Lead）做最终裁决，给出建议立场与理由`
}

function openTableZh(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」开放圆桌讨论。

# 用户提出的议题
${topic}

# 执行要求
1. 你是主持人（Lead）。先判断议题需要哪些专业视角：
   - 若用户已在议题里点名了具体角色，优先采用
   - 否则你自行召集 3-5 位合适的专家
2. 在首条回复里**明确告知用户名单**（名字 + 一句话身份），让用户知晓召集到的专家
3. 调用 TeamCreate 创建团队（team_name = "${runtimeTeamName(team)}"，这是工具内部稳定名称；对用户展示仍使用「${team.name}」）
4. 用 Agent 工具为每位专家 spawn 子代理，注入 name + persona
5. spawn 时给每位专家一个稳定的 name（小写英文 / kebab-case，如 ceo、growth-hacker），并保持后续轮次复用同一 name
6. 必须全程使用中文回复用户；团队名称、阶段说明、总结句都使用中文，不要夹带英文团队名
7. 让每位专家就议题发言一轮（每人 200-400 字），你串场并最终汇总观点`
}

function roundsEn(team: ExpertTeam, topic: string): string {
  return `Your task is to host a "${team.name}" roundtable discussion for the user.

# Team members
${renderRoster(team, 'en-US')}

# User topic
${topic}

# Requirements
1. Call TeamCreate to create the team (team_name = "${runtimeTeamName(team)}"; this is the stable internal tool name. Display "${team.name}" to the user.)
2. Spawn one sub-agent for each expert above with the Agent tool, injecting the expert name and persona into each system prompt.
3. ${spawnNameRule('en-US')}
4. Reply to the user in English throughout; team names, progress narration, and final summaries must be English.
5. Let every expert give one round of views on the topic (200-400 words each), respond to each other, then identify consensus and disagreement.
6. As the Lead, summarize the final decision recommendation for the user.`
}

function debateEn(team: ExpertTeam, topic: string): string {
  return `Your task is to host a structured debate for the user with "${team.name}".

# Team members
${renderRoster(team, 'en-US')}

# Debate topic
${topic}

# Requirements
1. Call TeamCreate to create the team (team_name = "${runtimeTeamName(team)}"; this is the stable internal tool name. Display "${team.name}" to the user.)
2. Spawn the affirmative, negative, and observer sub-agents with the Agent tool, injecting each persona into the system prompt.
3. ${spawnNameRule('en-US')}
4. Reply to the user in English throughout; team names, progress narration, and final summaries must be English.
5. Run 2-3 rounds:
   - Affirmative statement -> negative statement
   - Negative cross-examines affirmative -> affirmative responds
   - Observer critiques the strength of both sides
6. As the Lead, make the final call and give the recommended position with reasons.`
}

function openTableEn(team: ExpertTeam, topic: string): string {
  return `Your task is to host an open roundtable discussion for the user with "${team.name}".

# User topic
${topic}

# Requirements
1. You are the Lead. First decide which professional perspectives the topic needs:
   - If the user named specific roles in the topic, prioritize those roles
   - Otherwise invite 3-5 suitable experts yourself
2. In the first reply, clearly tell the user the invited expert list (name + one-line identity).
3. Call TeamCreate to create the team (team_name = "${runtimeTeamName(team)}"; this is the stable internal tool name. Display "${team.name}" to the user.)
4. Spawn one sub-agent for each expert with the Agent tool, injecting name + persona.
5. Give every expert a stable spawn name (lowercase English / kebab-case, such as ceo or growth-hacker), and reuse the same name in later rounds.
6. Reply to the user in English throughout; team names, progress narration, and final summaries must be English.
7. Let every expert speak once on the topic (200-400 words each), connect the discussion as Lead, and summarize the final views.`
}

export function buildDirectorPrompt(
  team: ExpertTeam,
  userTopic: string,
  language?: string,
): string {
  const locale = normalizePromptLocale(language)
  const promptTeam = teamForPrompt(team, locale)
  const topic = userTopic.trim()
  const template = promptTeam.directorPromptTemplate?.trim()
  if (template) {
    return renderDirectorPromptTemplate(template, promptTeam, topic, locale)
  }
  if (locale === 'en-US') {
    switch (promptTeam.facilitationStyle) {
      case 'debate':
        return debateEn(promptTeam, topic)
      case 'open':
        return openTableEn(promptTeam, topic)
      case 'rounds':
      default:
        return roundsEn(promptTeam, topic)
    }
  }
  switch (promptTeam.facilitationStyle) {
    case 'debate':
      return debateZh(promptTeam, topic)
    case 'open':
      return openTableZh(promptTeam, topic)
    case 'rounds':
    default:
      return roundsZh(promptTeam, topic)
  }
}
