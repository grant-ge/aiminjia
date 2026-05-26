// code/src/features/expert-teams/teams.ts
// 内置专家团 — 单一真相源。任何 UI / prompt 渲染从此读取。
// MVP 仅中文；不做 i18n，prompt 也是中文。

import i18n from '@/i18n'
import type { ExpertTeamSnapshot } from '@/lib/tauri'

export type ExpertTeamId = string

export type FacilitationStyle = 'rounds' | 'debate' | 'open'

export interface ExpertPersona {
  /** 角色名，会被注入 sub-agent system prompt */
  name: string
  /** Runtime teammate name emitted by Team events. UI keeps this name visible but can use it for avatar lookup. */
  agentName?: string
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
  snapshot?: ExpertTeamSnapshot
}

export const BUILTIN_EXPERT_TEAMS: ExpertTeam[] = [
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

export const EXPERT_TEAMS = BUILTIN_EXPERT_TEAMS

export function snapshotToExpertTeam(snapshot: ExpertTeamSnapshot, language?: string): ExpertTeam {
  const lang = (language ?? i18n.language) === 'en-US' ? 'en-US' : 'zh-CN'
  const display = snapshot.displayI18n[lang] ?? snapshot.displayI18n['zh-CN'] ?? {
    name: snapshot.teamId,
  }
  return {
    id: snapshot.teamId,
    name: display.name,
    emoji: snapshot.experts[0]?.emoji ?? '🧠',
    tagline: display.tagline ?? '',
    experts: snapshot.experts.map((expert) => ({
      name: expert.displayI18n?.[lang]?.name ?? expert.displayI18n?.['zh-CN']?.name ?? expert.stableName,
      agentName: expert.stableName,
      persona: expert.promptI18n?.[lang]?.persona ?? expert.promptI18n?.['zh-CN']?.persona ?? '',
      emoji: expert.emoji ?? '🧠',
    })),
    examples: display.examples ?? [],
    composerPlaceholder: display.composerPlaceholder ?? '',
    facilitationStyle: snapshot.facilitationStyle,
    snapshot,
  }
}

export function getExpertTeam(id: ExpertTeamId): ExpertTeam | undefined {
  return EXPERT_TEAMS.find((t) => t.id === id)
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
