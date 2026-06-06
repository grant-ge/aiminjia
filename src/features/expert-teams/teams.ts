// code/src/features/expert-teams/teams.ts
// Legacy expert-team definitions.
// The expert-team marketplace is server-authoritative; this file remains only
// for existing conversations and tests that reference the original built-ins.

export type ExpertTeamId = string

export type FacilitationStyle = 'rounds' | 'debate' | 'open'

export interface ExpertAvatarAtlas {
  kind: 'atlas'
  url: string
  x: number
  y: number
  w: number
  h: number
  atlasWidth: number
  atlasHeight: number
}

export type ExpertAvatarSource = string | ExpertAvatarAtlas

export interface ExpertPersona {
  /** 角色名，会被注入 sub-agent system prompt */
  name: string
  /** Stable source name used for local avatar filenames when display name is localized. */
  avatarName?: string
  /** Runtime teammate name emitted by Team events. UI keeps this name visible but can use it for avatar lookup. */
  agentName?: string
  /** Server-provided avatar. Existing remote teams use an OSS atlas; newer rows may provide avatar text. */
  avatar?: ExpertAvatarSource | null
  /** Short text fallback when no image avatar is available. */
  avatarText?: string | null
  /** 简短 persona，描述风格 / 关注点 */
  persona: string
  /**
   * 角色头像 emoji（单字符）。用于 ExpertTeamCard 的成员头像组、
   * 进入会话后子代理回复气泡的圆形头像。MVP 用 emoji，后续可平滑
   * 升级为真人头像（向 SubAgentResultCard 传 avatarUrl 即可）。
   */
  emoji: string
}

export interface ExpertTeam {
  id: ExpertTeamId
  name: string
  emoji: string
  tagline: string
  /** 团队成员。`roundtable` 为空数组，主持人按议题动态召集。 */
  experts: ExpertPersona[]
  /** 卡片底部展示的示例话题 chip */
  examples: string[]
  /** 进入会话后的 composer placeholder */
  composerPlaceholder: string
  /** 决定 buildDirectorPrompt 的模板分支 */
  facilitationStyle: FacilitationStyle
  /** Optional server-authored director prompt template with {{teamName}} style variables. */
  directorPromptTemplate?: string | null
  /** Server-side workplace directory category metadata. */
  workplaceCategoryId?: string | null
  workplaceCategoryName?: string | null
  workplaceCategoryDescription?: string | null
  workplaceCategoryIcon?: string | null
  workplaceCategoryColor?: string | null
  workplaceCategorySortOrder?: number | null
  sortOrder?: number | null
}

export const EXPERT_TEAMS: ExpertTeam[] = [
  {
    id: 'marketing',
    name: '市场营销策划团',
    emoji: '📣',
    tagline: '发布会 / 营销活动 / 市场策略',
    experts: [
      { name: '品牌负责人', agentName: 'brand-lead', persona: '关注定位、调性、长期心智占领', emoji: '🎨' },
      { name: '内容主理人', agentName: 'content-lead', persona: '善用故事和情绪共鸣，关注转化文案', emoji: '✍️' },
      { name: '增长黑客', agentName: 'growth-hacker', persona: '数据驱动，关注漏斗与 ROI 实验', emoji: '📈' },
      { name: '渠道经理', agentName: 'channel-manager', persona: '熟悉主流投放渠道与媒介组合', emoji: '📡' },
    ],
    examples: ['策划一场新品发布会', '618 大促营销节奏怎么排'],
    composerPlaceholder: '告诉他们你想策划什么活动…',
    facilitationStyle: 'rounds',
  },
  {
    id: 'operations',
    name: '经营决策团',
    emoji: '📊',
    tagline: '报表评审 / 经营决策 / 预算分配',
    experts: [
      { name: 'CEO', agentName: 'ceo', persona: '统筹全局，平衡短期收益与长期战略', emoji: '🧭' },
      { name: 'CFO', agentName: 'cfo', persona: '关注现金流、毛利、单位经济模型', emoji: '💰' },
      { name: 'COO', agentName: 'coo', persona: '关注执行效率、组织协同、流程瓶颈', emoji: '⚙️' },
      { name: '数据分析师', agentName: 'analyst', persona: '用数字说话，拆指标、找异动归因', emoji: '📊' },
    ],
    examples: ['Q2 经营数据下滑怎么看', '明年预算怎么分配'],
    composerPlaceholder: '告诉他们你想评审什么决策…',
    facilitationStyle: 'rounds',
  },
  {
    id: 'strategy',
    name: '战略推演团',
    emoji: '🎯',
    tagline: '重大决策前的多视角压力测试',
    experts: [
      { name: '战略顾问', agentName: 'strategy-advisor', persona: '麦肯锡式严谨，擅长 SWOT / 五力分析', emoji: '🧠' },
      { name: 'CFO', agentName: 'cfo', persona: '关注 ROI、现金流、风险敞口', emoji: '💰' },
      { name: '法务总监', agentName: 'legal-director', persona: '关注合规、合同、监管风险', emoji: '⚖️' },
      { name: 'CEO 教练', agentName: 'ceo-coach', persona: '善于反问、暴露盲点', emoji: '🪞' },
    ],
    examples: ['是否拓展东南亚市场', '是否启动 B 轮融资'],
    composerPlaceholder: '告诉他们你想推演什么决策…',
    facilitationStyle: 'rounds',
  },
  {
    id: 'negotiation',
    name: '沟通/谈判预演团',
    emoji: '🤝',
    tagline: '难谈话陪练',
    experts: [
      { name: '沟通教练', agentName: 'comm-coach', persona: '关注措辞、节奏、情绪管理', emoji: '🎤' },
      { name: '异议方角色', agentName: 'opponent', persona: '扮演对方立场，给出真实反驳', emoji: '🛡️' },
      { name: '第三方观察', agentName: 'observer', persona: '中立复盘强弱点', emoji: '👁️' },
      { name: '我方代表', agentName: 'self-rep', persona: '准备主张并接受指导', emoji: '🙋' },
    ],
    examples: ['跟核心员工谈降薪', '跟供应商谈降价 20%'],
    composerPlaceholder: '告诉他们你要预演什么对话…',
    facilitationStyle: 'rounds',
  },
  {
    id: 'retrospective',
    name: '复盘归因团',
    emoji: '🔍',
    tagline: '失败项目复盘 / 数据下滑归因',
    experts: [
      { name: '业务负责人', agentName: 'business-lead', persona: '熟悉一线场景，能给具体决策上下文', emoji: '🧑‍💼' },
      { name: '数据分析师', agentName: 'analyst', persona: '拆指标、找异动归因', emoji: '📊' },
      { name: 'HR', agentName: 'hr', persona: '关注组织和人的因素', emoji: '🤝' },
      { name: '流程顾问', agentName: 'process-advisor', persona: '关注 SOP、协作链路的断点', emoji: '🔗' },
    ],
    examples: ['上季度某产品线为何不及预期', '新人 30 天流失率为何上升'],
    composerPlaceholder: '告诉他们你想复盘什么事件…',
    facilitationStyle: 'rounds',
  },
  {
    id: 'investment',
    name: '投资评估团',
    emoji: '💼',
    tagline: '并购 / 新业务 / 投资标的尽调',
    experts: [
      { name: '资深投资人', agentName: 'investor', persona: '看赛道、看团队、看估值合理性', emoji: '💎' },
      { name: 'CFO', agentName: 'cfo', persona: '看财务模型与现金流敏感性', emoji: '💰' },
      { name: '行业专家', agentName: 'industry-expert', persona: '看竞争格局与技术壁垒', emoji: '🔬' },
      { name: '风控总监', agentName: 'risk-director', persona: '看合规、法律、退出风险', emoji: '🛡️' },
    ],
    examples: ['是否投资某 AI 教育公司', '收购 X 团队的代价值不值'],
    composerPlaceholder: '告诉他们你要评估什么标的…',
    facilitationStyle: 'rounds',
  },
  {
    id: 'debate',
    name: '辩论团',
    emoji: '⚖️',
    tagline: '两难决策 / 是否型选择',
    experts: [
      { name: '正方', agentName: 'pro', persona: '论证「应该」的立场，给出最强支持论据', emoji: '👍' },
      { name: '反方', agentName: 'con', persona: '论证「不应该」的立场，给出最强反对论据', emoji: '👎' },
      { name: '主持人', agentName: 'moderator', persona: '组织流程、控制时间、最终裁决', emoji: '🎙️' },
      { name: '观察员', agentName: 'observer', persona: '事后点评双方论点的强弱', emoji: '🔍' },
    ],
    examples: ['是否引入 AI 全员替换初级岗', '是否砍掉亏损但情怀项目'],
    composerPlaceholder: '告诉他们你想辩什么题…',
    facilitationStyle: 'debate',
  },
  {
    id: 'roundtable',
    name: '圆桌讨论团',
    emoji: '🪑',
    tagline: '开放议题 / 不确定角色构成',
    experts: [],
    examples: ['团队五年后的工作形态会是怎样', '中小企业如何拥抱 AI'],
    composerPlaceholder: '抛出你的议题，主持人会召集合适的专家…',
    facilitationStyle: 'open',
  },
]

type ExpertTeamLocale = 'zh-CN' | 'en-US'
type ExpertTeamText = Pick<ExpertTeam, 'name' | 'tagline' | 'examples' | 'composerPlaceholder'> & {
  experts?: Array<Pick<ExpertPersona, 'name' | 'persona'>>
}

const EXPERT_TEAM_I18N: Record<string, Partial<Record<ExpertTeamLocale, ExpertTeamText>>> = {
  marketing: {
    'en-US': {
      name: 'Marketing Planning Team',
      tagline: 'Launches, campaigns, and market strategy',
      examples: ['Plan a new product launch', 'Build a promotion timeline for 618'],
      composerPlaceholder: 'Tell the team what campaign you want to plan...',
      experts: [
        { name: 'Brand Lead', persona: 'Focuses on positioning, tone, and long-term mindshare' },
        { name: 'Content Lead', persona: 'Uses stories and emotion to improve conversion copy' },
        { name: 'Growth Hacker', persona: 'Data-driven, focused on funnels and ROI experiments' },
        { name: 'Channel Manager', persona: 'Understands media mix and mainstream acquisition channels' },
      ],
    },
  },
  operations: {
    'en-US': {
      name: 'Business Decision Team',
      tagline: 'Report reviews, operating decisions, and budget allocation',
      examples: ['Analyze why Q2 metrics declined', 'Decide next year budget allocation'],
      composerPlaceholder: 'Tell the team what decision you want to review...',
      experts: [
        { name: 'CEO', persona: 'Balances short-term returns with long-term strategy' },
        { name: 'CFO', persona: 'Focuses on cash flow, gross margin, and unit economics' },
        { name: 'COO', persona: 'Focuses on execution efficiency, collaboration, and process bottlenecks' },
        { name: 'Data Analyst', persona: 'Uses numbers to break down metrics and explain anomalies' },
      ],
    },
  },
  strategy: {
    'en-US': {
      name: 'Strategy Simulation Team',
      tagline: 'Multi-perspective stress tests before major decisions',
      examples: ['Should we expand into Southeast Asia?', 'Should we start Series B fundraising?'],
      composerPlaceholder: 'Tell the team what decision you want to simulate...',
      experts: [
        { name: 'Strategy Advisor', persona: 'Rigorous consultant style, strong at SWOT and Five Forces' },
        { name: 'CFO', persona: 'Focuses on ROI, cash flow, and risk exposure' },
        { name: 'Legal Director', persona: 'Focuses on compliance, contracts, and regulatory risk' },
        { name: 'CEO Coach', persona: 'Asks sharp questions and exposes blind spots' },
      ],
    },
  },
  negotiation: {
    'en-US': {
      name: 'Communication and Negotiation Team',
      tagline: 'Practice for difficult conversations',
      examples: ['Discuss salary cuts with a key employee', 'Negotiate a 20% supplier discount'],
      composerPlaceholder: 'Tell the team what conversation you want to rehearse...',
      experts: [
        { name: 'Communication Coach', persona: 'Focuses on wording, pacing, and emotional control' },
        { name: 'Opposing Role', persona: 'Represents the other side and gives realistic objections' },
        { name: 'Neutral Observer', persona: 'Reviews strengths and weaknesses from a neutral stance' },
        { name: 'Our Representative', persona: 'Prepares our claims and accepts coaching' },
      ],
    },
  },
  retrospective: {
    'en-US': {
      name: 'Retrospective Diagnosis Team',
      tagline: 'Failed project reviews and metric decline diagnosis',
      examples: ['Why did a product line miss expectations last quarter?', 'Why did 30-day new hire churn rise?'],
      composerPlaceholder: 'Tell the team what event you want to review...',
      experts: [
        { name: 'Business Lead', persona: 'Adds frontline context and concrete decision history' },
        { name: 'Data Analyst', persona: 'Breaks down metrics and finds anomaly drivers' },
        { name: 'HR', persona: 'Focuses on organizational and people factors' },
        { name: 'Process Advisor', persona: 'Focuses on SOPs and collaboration breakdowns' },
      ],
    },
  },
  investment: {
    'en-US': {
      name: 'Investment Evaluation Team',
      tagline: 'Due diligence for M&A, new businesses, and investments',
      examples: ['Should we invest in an AI education company?', 'Is acquiring team X worth the cost?'],
      composerPlaceholder: 'Tell the team what target you want to evaluate...',
      experts: [
        { name: 'Senior Investor', persona: 'Evaluates market, team, and valuation reasonableness' },
        { name: 'CFO', persona: 'Examines financial models and cash-flow sensitivity' },
        { name: 'Industry Expert', persona: 'Evaluates competitive landscape and technical moats' },
        { name: 'Risk Director', persona: 'Evaluates compliance, legal, and exit risks' },
      ],
    },
  },
  debate: {
    'en-US': {
      name: 'Debate Team',
      tagline: 'Binary choices and dilemma decisions',
      examples: ['Should AI replace all junior roles?', 'Should we shut down an unprofitable passion project?'],
      composerPlaceholder: 'Tell the team what topic you want to debate...',
      experts: [
        { name: 'Affirmative', persona: 'Argues the strongest case for yes' },
        { name: 'Negative', persona: 'Argues the strongest case against' },
        { name: 'Moderator', persona: 'Runs the process, manages time, and makes the final call' },
        { name: 'Observer', persona: 'Reviews the strengths and weaknesses of both sides' },
      ],
    },
  },
  roundtable: {
    'en-US': {
      name: 'Roundtable Team',
      tagline: 'Open topics with flexible expert roles',
      examples: ['What will work look like in five years?', 'How should small businesses adopt AI?'],
      composerPlaceholder: 'Share a topic and the director will invite suitable experts...',
      experts: [],
    },
  },
}

const remoteExpertTeams = new Map<ExpertTeamId, ExpertTeam>()

function normalizeLocale(language?: string): ExpertTeamLocale {
  return language?.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

export function localizeExpertTeam(team: ExpertTeam, language?: string): ExpertTeam {
  const locale = normalizeLocale(language)
  const text = EXPERT_TEAM_I18N[team.id]?.[locale]
  if (!text) return team
  return {
    ...team,
    name: text.name,
    tagline: text.tagline,
    examples: text.examples,
    composerPlaceholder: text.composerPlaceholder,
    experts: team.experts.map((expert, index) => {
      const localized = text.experts?.[index]
      if (!localized) return expert
      return {
        ...expert,
        avatarName: expert.avatarName ?? expert.name,
        name: localized.name,
        persona: localized.persona,
      }
    }),
  }
}

export function getExpertTeams(language?: string): ExpertTeam[] {
  return EXPERT_TEAMS.map((team) => localizeExpertTeam(team, language))
}

export function getExpertTeam(id: ExpertTeamId, language?: string): ExpertTeam | undefined {
  const remote = remoteExpertTeams.get(id)
  if (remote) return remote
  const team = EXPERT_TEAMS.find((t) => t.id === id)
  return team ? localizeExpertTeam(team, language) : undefined
}

export function setRemoteExpertTeams(teams: ExpertTeam[]) {
  remoteExpertTeams.clear()
  for (const team of teams) {
    remoteExpertTeams.set(team.id, team)
  }
}

function normalizeExpertKey(value: string): string {
  return value.toLowerCase().replace(/[\s\-_]+/g, '')
}

export function findExpertByAgentName(
  team: ExpertTeam | null | undefined,
  agentName: string,
): ExpertPersona | null {
  if (!team) return null
  let expert = team.experts.find((e) => e.agentName === agentName || e.name === agentName)
  if (expert) return expert

  const target = normalizeExpertKey(agentName)
  expert = team.experts.find((e) => {
    const byName = normalizeExpertKey(e.name) === target
    const byAgent = e.agentName ? normalizeExpertKey(e.agentName) === target : false
    const byAvatarName = e.avatarName ? normalizeExpertKey(e.avatarName) === target : false
    return byName || byAgent || byAvatarName
  })
  return expert ?? null
}

export function getExpertDisplayName(
  team: ExpertTeam | null | undefined,
  agentName: string,
): string {
  return findExpertByAgentName(team, agentName)?.name ?? agentName
}

/**
 * Resolve an expert name (from a subagent message) to an emoji. Tries:
 * 1. exact name match in the team's experts list (for `rounds` / `debate`)
 * 2. fallback to the team emoji (open-table where experts are dynamic
 *    and the team facilitates ad-hoc roles)
 *
 * Returns null when neither lookup hits — caller can use ChatAvatar's
 * initial fallback in that case.
 */
export function getExpertEmoji(team: ExpertTeam, expertName: string): string | null {
  const match = team.experts.find((e) => e.name === expertName)
  if (match) return match.emoji
  // Open-table teams have empty experts[]; team emoji is the right
  // generic fallback for ad-hoc roles spawned by the director.
  if (team.experts.length === 0) return team.emoji
  return null
}
