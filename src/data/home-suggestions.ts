export interface HomeExpertCategory {
  key: string
  label: string
  icon: 'sparkles' | 'pencil' | 'search' | 'file' | 'chart' | 'bot'
}

export interface HomeSuggestionItem {
  key: string
  title: string
  desc: string
  prompt: string
  skillId?: string
}

export const HOME_EXPERT_CATEGORIES: HomeExpertCategory[] = [
  { key: 'recommend', label: '为你推荐', icon: 'sparkles' },
  { key: 'planning', label: '规划专家', icon: 'pencil' },
  { key: 'research', label: '研究专家', icon: 'search' },
  { key: 'docs', label: '文档专家', icon: 'file' },
  { key: 'analysis', label: '分析专家', icon: 'chart' },
  { key: 'automation', label: '自动化专家', icon: 'bot' },
]

export const HOME_SUGGESTIONS: Record<string, HomeSuggestionItem[]> = {
  recommend: [
    {
      key: 'kickoff-plan',
      title: '实施计划',
      desc: '帮我把这个项目目标拆成 4 个阶段，列出每阶段交付物、负责人和风险。',
      prompt: '帮我把这个项目目标拆成 4 个阶段，列出每阶段交付物、负责人和风险。',
      skillId: 'writing-plans',
    },
    {
      key: 'research-brief',
      title: '行业调研',
      desc: '围绕这个主题做一版调研摘要，包含趋势、竞品、机会点和建议动作。',
      prompt: '围绕这个主题做一版调研摘要，包含趋势、竞品、机会点和建议动作。',
      skillId: 'research-brief',
    },
    {
      key: 'sheet-review',
      title: '表格分析',
      desc: '我有一份 Excel/CSV，帮我找出关键指标、异常波动和需要解释的变化。',
      prompt: '我有一份 Excel/CSV，帮我找出关键指标、异常波动和需要解释的变化。',
      skillId: 'table-analysis',
    },
    {
      key: 'ppt-outline',
      title: '汇报提纲',
      desc: '根据这个项目背景，给我一版汇报 PPT 的结构提纲和每页要表达的重点。',
      prompt: '根据这个项目背景，给我一版汇报 PPT 的结构提纲和每页要表达的重点。',
      skillId: 'ppt-builder',
    },
  ],
  planning: [
    {
      key: 'weekly-plan',
      title: '周计划拆解',
      desc: '把这周的目标拆成每日任务，标注优先级、依赖关系和验收标准。',
      prompt: '把这周的目标拆成每日任务，标注优先级、依赖关系和验收标准。',
      skillId: 'writing-plans',
    },
    {
      key: 'roadmap',
      title: '路线图',
      desc: '给我一版季度路线图，按里程碑列出关键动作、资源需求和潜在阻塞。',
      prompt: '给我一版季度路线图，按里程碑列出关键动作、资源需求和潜在阻塞。',
      skillId: 'writing-plans',
    },
    {
      key: 'retro',
      title: '复盘框架',
      desc: '帮我整理一版复盘模板，按目标、结果、问题、改进动作来组织。',
      prompt: '帮我整理一版复盘模板，按目标、结果、问题、改进动作来组织。',
      skillId: 'writing-plans',
    },
    {
      key: 'meeting-followup',
      title: '会后行动项',
      desc: '把这段会议内容整理成行动项清单，写清 owner、截止时间和下一步。',
      prompt: '把这段会议内容整理成行动项清单，写清 owner、截止时间和下一步。',
      skillId: 'writing-plans',
    },
  ],
  research: [
    {
      key: 'competitor',
      title: '竞品对比',
      desc: '帮我对比 3 个竞品的核心功能、定价策略、目标用户和差异化优势。',
      prompt: '帮我对比 3 个竞品的核心功能、定价策略、目标用户和差异化优势。',
      skillId: 'research-brief',
    },
    {
      key: 'trend-scan',
      title: '趋势扫描',
      desc: '围绕这个行业做趋势扫描，总结值得关注的新变化和对业务的影响。',
      prompt: '围绕这个行业做趋势扫描，总结值得关注的新变化和对业务的影响。',
      skillId: 'research-brief',
    },
    {
      key: 'user-insight',
      title: '用户洞察',
      desc: '根据我给的信息，整理目标用户画像、痛点、决策因素和沟通策略。',
      prompt: '根据我给的信息，整理目标用户画像、痛点、决策因素和沟通策略。',
      skillId: 'research-brief',
    },
    {
      key: 'source-summary',
      title: '资料速读',
      desc: '把这批材料读完后，输出一页纸摘要，标记重点结论和待确认问题。',
      prompt: '把这批材料读完后，输出一页纸摘要，标记重点结论和待确认问题。',
      skillId: 'research-brief',
    },
  ],
  docs: [
    {
      key: 'prd-template',
      title: '需求文档',
      desc: '根据这个需求想法，写一版 PRD 模板，包含目标、流程、边界和验收标准。',
      prompt: '根据这个需求想法，写一版 PRD 模板，包含目标、流程、边界和验收标准。',
      skillId: 'writing-plans',
    },
    {
      key: 'ppt-structure',
      title: '演示文稿',
      desc: '帮我设计一个 10 页以内的演示文稿结构，并写出每页标题和核心信息。',
      prompt: '帮我设计一个 10 页以内的演示文稿结构，并写出每页标题和核心信息。',
      skillId: 'ppt-builder',
    },
    {
      key: 'sop',
      title: 'SOP 流程',
      desc: '把这个流程沉淀成 SOP，按步骤、输入、输出、注意事项来整理。',
      prompt: '把这个流程沉淀成 SOP，按步骤、输入、输出、注意事项来整理。',
      skillId: 'writing-plans',
    },
    {
      key: 'reply-draft',
      title: '专业回复',
      desc: '根据这段上下文，帮我起草一版专业、清晰、可直接发送的回复文案。',
      prompt: '根据这段上下文，帮我起草一版专业、清晰、可直接发送的回复文案。',
    },
  ],
  analysis: [
    {
      key: 'sales-report',
      title: '经营复盘',
      desc: '分析这份经营数据，找出增长点、异常项和下阶段最值得跟进的动作。',
      prompt: '分析这份经营数据，找出增长点、异常项和下阶段最值得跟进的动作。',
      skillId: 'table-analysis',
    },
    {
      key: 'cohort',
      title: '指标拆解',
      desc: '帮我拆解核心指标的影响因素，并给出排查思路和验证方法。',
      prompt: '帮我拆解核心指标的影响因素，并给出排查思路和验证方法。',
      skillId: 'table-analysis',
    },
    {
      key: 'summary-table',
      title: '表格汇总',
      desc: '把这批表格数据整理成一版结论摘要，重点突出趋势、分层和异常值。',
      prompt: '把这批表格数据整理成一版结论摘要，重点突出趋势、分层和异常值。',
      skillId: 'table-analysis',
    },
    {
      key: 'dashboard',
      title: '看板建议',
      desc: '如果我要做一个业务看板，应该关注哪些指标、维度和预警阈值？',
      prompt: '如果我要做一个业务看板，应该关注哪些指标、维度和预警阈值？',
      skillId: 'table-analysis',
    },
  ],
  automation: [
    {
      key: 'workflow-idea',
      title: '流程自动化',
      desc: '把这项重复工作整理成可执行流程，拆出触发条件、步骤和异常分支。',
      prompt: '把这项重复工作整理成可执行流程，拆出触发条件、步骤和异常分支。',
      skillId: 'writing-plans',
    },
    {
      key: 'automation-skill',
      title: '定制技能',
      desc: '帮我设计一个技能，让团队成员可以稳定复用这套任务处理流程。',
      prompt: '帮我设计一个技能，让团队成员可以稳定复用这套任务处理流程。',
      skillId: 'writing-plans',
    },
    {
      key: 'agent-brief',
      title: '执行说明',
      desc: '我想把这件事交给 AI 长流程执行，先帮我写清输入、约束、检查点和输出。',
      prompt: '我想把这件事交给 AI 长流程执行，先帮我写清输入、约束、检查点和输出。',
      skillId: 'writing-plans',
    },
    {
      key: 'handoff',
      title: '协作交接',
      desc: '把这个复杂任务整理成交接说明，确保别人或 AI 接手后能继续推进。',
      prompt: '把这个复杂任务整理成交接说明，确保别人或 AI 接手后能继续推进。',
      skillId: 'writing-plans',
    },
  ],
}
