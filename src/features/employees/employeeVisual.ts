import type { EmployeeTemplate } from './templates'

const SAFE_RE = /[\\/<>:"|?*\s]/g
const RELEASE_RESOURCE_BASE_URL = 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/'
const LOCAL_EMPLOYEE_AVATAR_NAMES = new Set([
  '林知远',
  '陈景律',
  '周思齐',
  '许嘉宁',
  '丁若安',
  '赵明川',
  '何予周',
  '沈柏川',
  '顾承远',
  '韩可欣',
  '程砚舟',
  '方予衡',
  '陆时安',
  '秦砚知',
  '温嘉言',
  '梁承序',
  '何远策',
  '唐识衡',
])

interface EmployeePersona {
  name: string
  title: string
  accent: string
  strengths: string[]
  examples: string[]
}

export interface EmployeeVisual {
  name: string
  title: string
  avatarUrl: string | null
  avatarText: string
  accent: string
  strengths: string[]
  examples: string[]
}

const EMPLOYEE_PERSONAS: Record<string, EmployeePersona> = {
  'builtin:xiaoyuan': {
    name: '林知远',
    title: '行业情报分析师',
    accent: 'bg-sky-50 text-sky-700',
    strengths: ['竞品动态监测', '渠道信息去重', '周报结构化输出'],
    examples: ['梳理本周竞品发布与价格变化', '对比三家同行招聘和媒体信号', '生成一份行业趋势周报'],
  },
  'builtin:xiaofa': {
    name: '陈景律',
    title: '合同与合规顾问',
    accent: 'bg-violet-50 text-violet-700',
    strengths: ['风险条款识别', '合同改写建议', '合规关注点梳理'],
    examples: ['审阅这份采购合同的高风险条款', '把付款和违约责任改得更稳妥', '整理合同谈判前的风险清单'],
  },
  'builtin:xiaosuan': {
    name: '周思齐',
    title: '经营数据分析师',
    accent: 'bg-emerald-50 text-emerald-700',
    strengths: ['表格数据分析', '异常归因', '图表和报告生成'],
    examples: ['分析这份销售表的本月波动', '找出费用异常增长的原因', '生成经营分析报告和行动建议'],
  },
  'builtin:xiaoxiao': {
    name: '许嘉宁',
    title: '客户跟进专员',
    accent: 'bg-rose-50 text-rose-700',
    strengths: ['客户优先级判断', '跟进话术建议', '钉钉表格同步'],
    examples: ['帮我判断今天最该跟进哪些客户', '整理在谈客户的风险和下一步', '把确认后的跟进结果同步回表格'],
  },
  'builtin:xiaoding': {
    name: '丁若安',
    title: '办公协同助理',
    accent: 'bg-amber-50 text-amber-700',
    strengths: ['日程摘要', '待办梳理', '会议协调'],
    examples: ['汇总今天日程和待办重点', '帮我找一个合适的会议时间', '整理群聊里需要跟进的事项'],
  },
  'builtin:xiaozhao': {
    name: '赵明川',
    title: '招聘研究员',
    accent: 'bg-indigo-50 text-indigo-700',
    strengths: ['简历筛选', '岗位画像', '面试问题设计'],
    examples: ['筛选这批简历并按匹配度排序', '帮我写一个岗位 JD', '为候选人生成针对性面试问题'],
  },
  'builtin:xiaozhou': {
    name: '何予周',
    title: '周报撰写顾问',
    accent: 'bg-cyan-50 text-cyan-700',
    strengths: ['工作记录汇总', '结构化表达', '周报润色'],
    examples: ['汇总本周工作生成周报', '把这些事项整理成管理层可读版本', '补充本周风险和下周计划'],
  },
  'builtin:xiaobiao': {
    name: '沈柏川',
    title: '标书方案顾问',
    accent: 'bg-teal-50 text-teal-700',
    strengths: ['招标文件解析', '方案章节撰写', '投标文件结构化'],
    examples: ['解析这份招标文件的评分点', '按模板撰写技术方案章节', '整理投标文件目录和交付清单'],
  },
  'builtin:xiaogong': {
    name: '顾承远',
    title: '技术支持工程师',
    accent: 'bg-slate-100 text-slate-700',
    strengths: ['技术问题归类', '知识库检索', '回复草稿生成'],
    examples: ['整理客户群里的技术问题', '查找历史解法并生成回复草稿', '沉淀一份常见问题清单'],
  },
  'builtin:xiaoke': {
    name: '韩可欣',
    title: '客户支持专员',
    accent: 'bg-pink-50 text-pink-700',
    strengths: ['业务咨询分流', 'FAQ 话术', '客户沟通复盘'],
    examples: ['整理客户咨询并给出回复草稿', '把这段对话沉淀成 FAQ', '分析客户最关心的产品问题'],
  },
  'builtin:xiaocheng': {
    name: '程砚舟',
    title: '流程设计师',
    accent: 'bg-orange-50 text-orange-700',
    strengths: ['流程拆解', '技能沉淀', '重复任务标准化'],
    examples: ['把这个重复流程整理成可复用技能', '帮我拆解一项团队交付 SOP', '把口头经验改写成 SKILL.md'],
  },
}

function safeName(name: string): string {
  return name.replace(SAFE_RE, '_').replace(/^[._]+|[._]+$/g, '') || 'unnamed'
}

function fallbackExamples(template: EmployeeTemplate): string[] {
  if (template.examples && template.examples.length > 0) return template.examples
  const base = template.description.trim()
  return base ? [`围绕「${template.role}」安排一项具体任务`, base] : ['描述目标和上下文，让 TA 开始处理']
}

export function getLocalEmployeeAvatarUrl(name: string): string | null {
  const normalized = name.trim()
  if (!LOCAL_EMPLOYEE_AVATAR_NAMES.has(normalized)) return null
  return `/employee-avatars/${safeName(normalized)}.svg`
}

function templateAvatarUrl(template: EmployeeTemplate): string | null {
  const localUrl = getLocalEmployeeAvatarUrl(template.name)
  if (localUrl) return localUrl

  const explicitUrl = template.avatarUrl?.trim()
  if (explicitUrl) return explicitUrl

  const key = template.avatarAssetKey?.trim().replace(/^\/+/, '')
  if (!key) return null
  if (/^https?:\/\//i.test(key) || key.startsWith('/')) return key
  return `${RELEASE_RESOURCE_BASE_URL}${key}`
}

export function getEmployeeVisual(template: EmployeeTemplate): EmployeeVisual {
  const persona = EMPLOYEE_PERSONAS[template.templateId]
  if (!persona) {
    const avatarText = template.avatar.trim() || employeeInitial(template.name)
    return {
      name: template.name,
      title: template.role,
      avatarUrl: templateAvatarUrl(template),
      avatarText,
      accent: 'bg-muted text-muted-foreground',
      strengths: [],
      examples: fallbackExamples(template),
    }
  }
  return {
    name: persona.name,
    title: persona.title || template.role,
    avatarUrl: getLocalEmployeeAvatarUrl(persona.name),
    avatarText: '',
    accent: persona.accent,
    strengths: persona.strengths,
    examples: template.examples && template.examples.length > 0 ? template.examples : persona.examples,
  }
}

export function employeeInitial(name: string): string {
  return Array.from(name.trim())[0] ?? '员'
}
