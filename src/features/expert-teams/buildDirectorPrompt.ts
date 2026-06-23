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

function renderDebateRoster(team: ExpertTeam, locale: PromptLocale): string {
  const debateTeam = {
    ...team,
    experts: team.experts.filter((expert) => expert.agentName !== 'moderator'),
  }
  return renderRoster(debateTeam, locale)
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
  const rendered = Object.entries(values).reduce(
    (out, [key, value]) => out.split(`{{${key}}}`).join(value),
    template,
  )
  return `${rendered}

${localRuntimeGuard(team, topic, locale)}`
}

/** Common spawn convention reminder shared by all facilitation styles. */
function spawnNameRule(locale: PromptLocale): string {
  return locale === 'en-US'
    ? '**When spawning sub-agents, the name parameter must strictly use the value shown in `[name="..."]` (for example ceo, cfo, analyst). Do not invent translations or abbreviations.** The frontend uses this stable name to identify each expert; mismatches cause avatar and message attribution errors. Do not ask clarification questions before creating the team. If the topic lacks details, state concise assumptions and still create the team and spawn the experts. After all experts are spawned, stop and wait for real expert messages; do not use standalone Agent calls to simulate missing experts, and do not summarize before the real members have replied. Write the final report directly in the assistant response; do not use TaskCreate, TaskUpdate, or TaskOutput to record expert-team reports.'
    : '**spawn 子代理时，name 参数必须严格使用 `[name="..."]` 中给定的值（如 ceo、cfo、analyst），不要自创翻译或缩写**。这是前端识别每位专家的依据；不一致会导致头像、消息归属错乱。不要在创建团队前反问用户补充信息；如果议题细节不足，先明示简短假设并继续创建团队、召唤专家。召唤完全部专家后立刻停下等待真实专家消息，不要再用普通 Agent 模拟未回复专家，也不要在真实成员回复前总结。最终报告直接写在 assistant 正文里，不要用 TaskCreate、TaskUpdate 或 TaskOutput 记录专家团报告。'
}

function wantsSingleRound(topic: string, locale: PromptLocale): boolean {
  const normalized = topic.toLowerCase()
  return locale === 'en-US'
    ? /single round|one round|no cross[- ]?review|no debate/.test(normalized)
    : /只要一轮|一轮专家观点|不要互评|不需要互评|不要交叉点评|不需要交叉点评/.test(topic)
}

function localRuntimeGuard(team: ExpertTeam, topic: string, locale: PromptLocale): string {
  const singleRound = wantsSingleRound(topic, locale)
  const debateModeratorGuard =
    team.facilitationStyle === 'debate'
      ? locale === 'en-US'
        ? '\n- In debate teams, `moderator` is the Lead itself. Do not spawn or wait for a `moderator` teammate; wait only for the affirmative, negative, and observer peer-messages.'
        : '\n- 辩论团里的 `moderator` 代表 Lead 自己，不要 spawn 或等待 `moderator` 子代理；只等待正方、反方、观察员的 peer-message。'
      : ''
  if (locale === 'en-US') {
    return `# Local runtime constraints
- TeamCreate must use team_name = "${runtimeTeamName(team)}"; this is the stable internal team name. User-facing copy may still say "${team.name}".
- Team supports at most 4 teammates. Never invite or spawn more than 4 experts; if a template says 3-5, cap it at 4.
- ${spawnNameRule('en-US')}
- Reply to the user in English unless the user explicitly requests another language.
- If the user asks for one round only, do not start cross-review or a second round. ${singleRound ? 'The current topic asks for one round only: after every declared expert has sent one peer-message, write the final report directly and then call TeamDelete.' : 'Only start cross-review when the user explicitly asks for it.'}
- Do not print internal retry/status text such as "fixing message format" or "retrying"; tool retries are invisible implementation details.${debateModeratorGuard}`
  }
  return `# 本地运行约束（优先级高于上面的远端模板）
- TeamCreate 必须使用 team_name = "${runtimeTeamName(team)}"；这是工具内部稳定名称。对用户展示时仍可称呼「${team.name}」。
- Team 最多支持 4 个 Teammate。不要邀请或 spawn 超过 4 位专家；如果远端模板写 3-5 位，本地按最多 4 位执行。
- ${spawnNameRule('zh-CN')}
- 必须全程使用中文回复用户；团队名称、阶段说明、总结句都使用中文，不要夹带英文等待/status 句。
- 如果用户要求“只要一轮”，不要进入互评、交叉点评或第二轮。${singleRound ? '当前议题已经明确只要一轮：全部声明专家各自发来一次 peer-message 后，直接整理最终报告并调用 TeamDelete。' : '只有用户明确要求互评时，才可以开启交叉点评。'}
- 不要把“消息格式需要调整”“重新发送”“retry”等内部修正过程输出给用户；工具重试是实现细节。${debateModeratorGuard}`
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
5. 让每位专家就议题发表一轮观点（每人 200-400 字）；如果用户没有明确要求互评，不要开启交叉点评
6. 你作为主持人（Lead）整理最终决策建议，呈现给用户`
}

function debateZh(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」结构化辩论。

# 团队成员
${renderDebateRoster(team, 'zh-CN')}

# 用户提出的辩题
${topic}

# 执行要求
1. 调用 TeamCreate 创建团队（team_name = "${runtimeTeamName(team)}"，这是工具内部稳定名称；对用户展示仍使用「${team.name}」）
2. 用 Agent 工具 spawn 正方、反方、观察员三个子代理，把 persona 注入 system prompt
3. ${spawnNameRule('zh-CN')}
4. 必须全程使用中文回复用户；团队名称、阶段说明、总结句都使用中文，不要夹带英文团队名
5. "moderator" 代表 Lead 自己，不要 spawn 或等待 "moderator" 子代理
6. 按以下流程进行 2-3 轮：
   - 正方陈述 → 反方陈述
   - 反方质询正方 → 正方反驳
   - 观察员点评双方论点强弱
7. 你作为主持人（Lead）做最终裁决，给出建议立场与理由`
}

function openTableZh(team: ExpertTeam, topic: string): string {
  return `你现在的任务是为用户主持一场「${team.name}」开放圆桌讨论。

# 用户提出的议题
${topic}

# 执行要求
1. 你是主持人（Lead）。先判断议题需要哪些专业视角：
   - 若用户已在议题里点名了具体角色，优先采用
   - 否则你自行召集 3-4 位合适的专家（本地 Team 上限为 4 位）
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
${renderDebateRoster(team, 'en-US')}

# Debate topic
${topic}

# Requirements
1. Call TeamCreate to create the team (team_name = "${runtimeTeamName(team)}"; this is the stable internal tool name. Display "${team.name}" to the user.)
2. Spawn the affirmative, negative, and observer sub-agents with the Agent tool, injecting each persona into the system prompt.
3. ${spawnNameRule('en-US')}
4. Reply to the user in English throughout; team names, progress narration, and final summaries must be English.
5. "moderator" means the Lead itself; do not spawn or wait for a "moderator" teammate.
6. Run 2-3 rounds:
   - Affirmative statement -> negative statement
   - Negative cross-examines affirmative -> affirmative responds
   - Observer critiques the strength of both sides
7. As the Lead, make the final call and give the recommended position with reasons.`
}

function openTableEn(team: ExpertTeam, topic: string): string {
  return `Your task is to host an open roundtable discussion for the user with "${team.name}".

# User topic
${topic}

# Requirements
1. You are the Lead. First decide which professional perspectives the topic needs:
   - If the user named specific roles in the topic, prioritize those roles
   - Otherwise invite 3-4 suitable experts yourself (local Team limit is 4)
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
