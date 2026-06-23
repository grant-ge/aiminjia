// 内置员工模板"身份元数据"——只保留 EmployeeCard / HireWizard / localizeEmployeeDisplay
// 需要的最小字段（templateId / name / role / description / resourceConfigKind / avatar / badge）。
//
// 历史背景（2026-06）：之前 BUILTIN_TEMPLATES 兼任 HireWizard 的"离线兜底员工列表"，
// 每条带 systemPromptExtra / toolWhitelist / cron 等完整运行时数据。但 AIjia 是云端唯一
// 架构，没网 → 网关挂 → 员工跑不了，"离线兜底"是伪命题（CLAUDE.md 决策 11）。
//
// 因此：
// - HireWizard 不再 fallback 到 BUILTIN_TEMPLATES——cache 空就显示 loading / 重试 UI
// - 后端 templates_bootstrap.json 完全删除，catalog 只来自服务端缓存
// - BUILTIN_TEMPLATES 余下用途：① findTemplate(id) → resourceConfigKind 路由
//   ② localizeEmployeeDisplay 查 base zh-CN 值用于已招员工的 i18n 翻译
//   两者都不依赖运行时大字段，所以这里只留身份元数据。

import type {
  EmployeeTemplateSnapshot,
  WorkplaceDirectoryItem,
  WorkplaceDirectoryRequiredSkill,
} from '@/lib/tauri'

export type ResourceConfigKind = 'monitoring-urls' | 'sales-table' | 'weekly-report' | 'tech-support' | 'customer-support' | 'none'

export interface RequiresAttachmentSpec {
  /** Comma-separated extension list passed to the file picker, e.g. `.pdf,.docx`. */
  accept: string
  min: number
  max: number
}

export interface EmployeeTemplate {
  templateId: string
  version?: string | null
  avatar: string
  avatarAssetKey?: string | null
  avatarUrl?: string | null
  name: string
  role: string
  description: string
  toolWhitelist: string[]
  cron: string | null
  systemPromptExtra: string
  badge: string
  /** When set, dispatch_employee_run prepends "请第一步调用 load_skill('<id>')" to the user message. */
  defaultSkillId: string | null
  /** When set, dispatch flow opens a file picker before calling employee_trigger. */
  requiresAttachment: RequiresAttachmentSpec | null
  /** Drives which HireWizard resource config subcomponent is shown in step 3. */
  resourceConfigKind: ResourceConfigKind
  /** True when an employee with this templateId requires `dingtalk_status().connected === true` before dispatch. */
  requiresDingtalk: boolean
  /** Server-localized platform skills this digital employee expects to use. */
  requiredSkills?: WorkplaceDirectoryRequiredSkill[]
  /** Server-side example assignments surfaced in the rich detail view. */
  examples?: string[]
  /** Server-side workplace directory category metadata, used for catalog grouping/display. */
  workplaceCategoryId?: string | null
  workplaceCategoryName?: string | null
  /**
   * JSON Schema for instance config (PR6, 2026-05-10). When present and
   * non-empty, HireWizard step 3 renders a SchemaForm against this schema
   * instead of the legacy hardcoded `resourceConfigKind`-driven form.
   * BUILTIN_TEMPLATES leave this empty — they keep their hand-tuned forms.
   * Custom (`org:` / `private:`) templates published via OPS portal can
   * supply a schema and skip the hardcoded form path entirely.
   */
  resourceConfigSchema?: Record<string, unknown> | null
}

/**
 * 构造一条 BUILTIN_TEMPLATE 身份元数据。
 *
 * 把 systemPromptExtra / toolWhitelist / cron / defaultSkillId /
 * requiresAttachment / requiresDingtalk 全部填空 —— 这些字段的权威值
 * 是后端 employeeTemplateCatalog() 推下来的 snapshot，不在前端硬编码。
 * 只保留 findTemplate / localizeEmployeeDisplay 真正读到的几个字段。
 */
function builtin(meta: Pick<
  EmployeeTemplate,
  'templateId' | 'avatar' | 'name' | 'role' | 'description' | 'badge' | 'resourceConfigKind'
>): EmployeeTemplate {
  return {
    ...meta,
    toolWhitelist: [],
    cron: null,
    systemPromptExtra: '',
    defaultSkillId: null,
    requiresAttachment: null,
    requiresDingtalk: false,
  }
}

export const BUILTIN_TEMPLATES: EmployeeTemplate[] = [
  builtin({
    templateId: 'builtin:xiaoyuan', avatar: '🔍', name: '小研',
    role: '行业/竞品调研员',
    description: '每周汇总竞品和行业渠道的产品发布、定价、招聘、媒体报道四个维度的变化，去重后生成周报。',
    badge: '🟢 开箱即用', resourceConfigKind: 'monitoring-urls',
  }),
  builtin({
    templateId: 'builtin:xiaofa', avatar: '⚖️', name: '小法',
    role: '合同审阅员',
    description: '按 10 大风险条款扫描 PDF/DOCX 合同，输出风险标注与改写建议。',
    badge: '🟢 开箱即用', resourceConfigKind: 'none',
  }),
  builtin({
    templateId: 'builtin:xiaosuan', avatar: '📊', name: '小算',
    role: '数据分析员',
    description: '自动 EDA + 异常检测 + 图表 + 假设检验 + 报告/PPT，支持 Excel/CSV 数据。',
    badge: '🟢 开箱即用', resourceConfigKind: 'none',
  }),
  builtin({
    templateId: 'builtin:xiaoxiao', avatar: '💼', name: '小销',
    role: '客户跟进员',
    description: '每个工作日早上读钉钉 AI 表格中的在谈客户，按优先级判定今天该跟进谁，口述结果后反向同步表格。',
    badge: '🟠 需配置数据源', resourceConfigKind: 'sales-table',
  }),
  builtin({
    templateId: 'builtin:xiaoding', avatar: '📌', name: '小钉',
    role: '钉办助理',
    description: '每天早晨汇总日程/待办/群聊重点，按需找空闲时段约会议、用户确认后发消息。',
    badge: '🟡 需授权钉钉', resourceConfigKind: 'none',
  }),
  builtin({
    templateId: 'builtin:xiaozhao', avatar: '🎯', name: '小招',
    role: '招聘助理',
    description: '批量筛选简历并按匹配度排序，撰写岗位 JD，搜索候选人公开信息,生成针对性面试问题。',
    badge: '🟢 开箱即用', resourceConfigKind: 'none',
  }),
  builtin({
    templateId: 'builtin:xiaozhou', avatar: '📝', name: '小周',
    role: '周报撰写员',
    description: '每周五自动汇总本周钉钉日程、已完成待办、群聊关键讨论，生成结构化周报，呈现在对话中供你查看与编辑。',
    badge: '🟡 需授权钉钉', resourceConfigKind: 'weekly-report',
  }),
  builtin({
    templateId: 'builtin:xiaobiao', avatar: '📋', name: '小标',
    role: '标书撰写员',
    description: '解析招标文件与参考模板，按结构化工作流分章节撰写完整投标文件，自动套用模板风格导出 docx。',
    badge: '🟢 开箱即用', resourceConfigKind: 'none',
  }),
  builtin({
    templateId: 'builtin:xiaogong', avatar: '🔧', name: '小工',
    role: '技术支持',
    description: '定时扫描客户钉钉群的技术提问，查阅技术文档和历史工单经验，生成回复草稿供确认后发送。自动积累 Q&A 经验库。',
    badge: '🟠 需配置', resourceConfigKind: 'tech-support',
  }),
  builtin({
    templateId: 'builtin:xiaoke', avatar: '💬', name: '小客',
    role: '客服支持',
    description: '定时扫描客户钉钉群的业务咨询，查阅产品 FAQ 和历史对话经验，生成友好回复草稿。持续积累客服话术库。',
    badge: '🟠 需配置', resourceConfigKind: 'customer-support',
  }),
]

/** Look up the template that produced an employee, by `EmployeeRecord.templateId`. */
export function findTemplate(templateId: string | null | undefined): EmployeeTemplate | null {
  if (!templateId) return null
  return BUILTIN_TEMPLATES.find((t) => t.templateId === templateId) ?? null
}

type TemplateLocale = 'zh-CN' | 'en-US'
type EmployeeTemplateDisplay = Pick<
  EmployeeTemplate,
  'name' | 'role' | 'description' | 'badge' | 'avatarAssetKey' | 'avatarUrl'
>

const RELEASE_RESOURCE_BASE_URL = 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/'

const BUILTIN_TEMPLATE_I18N: Record<string, Partial<Record<TemplateLocale, Partial<EmployeeTemplateDisplay>>>> = {
  'builtin:xiaoyuan': {
    'en-US': {
      name: 'XiaoYan',
      role: 'Industry and competitor researcher',
      description: 'Tracks product launches, pricing, hiring, and media signals from competitors and industry channels, then deduplicates them into a weekly report.',
      badge: 'Ready',
    },
  },
  'builtin:xiaofa': {
    'en-US': {
      name: 'XiaoFa',
      role: 'Contract reviewer',
      description: 'Scans PDF/DOCX contracts for key risk clauses and produces risk notes with rewrite suggestions.',
      badge: 'Ready',
    },
  },
  'builtin:xiaosuan': {
    'en-US': {
      name: 'XiaoSuan',
      role: 'Data analyst',
      description: 'Runs EDA, anomaly checks, charts, hypothesis tests, and report generation for Excel/CSV datasets.',
      badge: 'Ready',
    },
  },
  'builtin:xiaoxiao': {
    'en-US': {
      name: 'XiaoXiao',
      role: 'Customer follow-up specialist',
      description: 'Reads active opportunities from DingTalk tables each workday, prioritizes follow-ups, and syncs confirmed results back.',
      badge: 'Needs data source',
    },
  },
  'builtin:xiaoding': {
    'en-US': {
      name: 'XiaoDing',
      role: 'DingTalk office assistant',
      description: 'Summarizes schedules, tasks, and group-chat highlights every morning, then helps book meetings or send confirmed messages.',
      badge: 'Needs DingTalk auth',
    },
  },
  'builtin:xiaozhao': {
    'en-US': {
      name: 'XiaoZhao',
      role: 'Recruiting assistant',
      description: 'Screens resumes in bulk, writes job descriptions, researches public candidate information, and generates tailored interview questions.',
      badge: 'Ready',
    },
  },
  'builtin:xiaozhou': {
    'en-US': {
      name: 'XiaoZhou',
      role: 'Weekly report writer',
      description: 'Summarizes DingTalk schedules, completed tasks, and key chat discussions every Friday into a structured weekly report.',
      badge: 'Needs DingTalk auth',
    },
  },
  'builtin:xiaobiao': {
    'en-US': {
      name: 'XiaoBiao',
      role: 'Bid proposal writer',
      description: 'Parses tender files and reference templates, drafts bid documents section by section, and exports docx files in the requested style.',
      badge: 'Ready',
    },
  },
  'builtin:xiaogong': {
    'en-US': {
      name: 'XiaoGong',
      role: 'Technical support assistant',
      description: 'Monitors support channels, searches the knowledge base and past cases, then drafts technical replies for review.',
      badge: 'Needs knowledge base',
    },
  },
  'builtin:xiaoke': {
    'en-US': {
      name: 'XiaoKe',
      role: 'Customer service assistant',
      description: 'Monitors customer inquiry channels, searches knowledge and past conversations, then drafts friendly replies for review.',
      badge: 'Needs knowledge base',
    },
  },
}

function normalizeLocale(language?: string): TemplateLocale {
  return language?.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

function selectTemplateDisplay(
  snap: EmployeeTemplateSnapshot,
  language?: string,
): Partial<EmployeeTemplateDisplay> {
  const locale = normalizeLocale(language)
  return (
    snap.displayI18n?.[locale] ??
    snap.displayI18n?.['zh-CN'] ??
    BUILTIN_TEMPLATE_I18N[snap.templateId]?.[locale] ??
    {}
  )
}

function selectTemplatePrompt(snap: EmployeeTemplateSnapshot, language?: string): string | undefined {
  const locale = normalizeLocale(language)
  return snap.promptI18n?.[locale]?.systemPromptExtra ?? snap.promptI18n?.['zh-CN']?.systemPromptExtra
}

function normalizeTemplateAvatarUrl(
  avatarUrl: string | null | undefined,
  avatarAssetKey: string | null | undefined,
): string | null {
  const explicitUrl = avatarUrl?.trim()
  if (explicitUrl) return explicitUrl

  const key = avatarAssetKey?.trim().replace(/^\/+/, '')
  if (!key) return null
  if (/^https?:\/\//i.test(key) || key.startsWith('/')) return key
  return `${RELEASE_RESOURCE_BASE_URL}${key}`
}

function matchesKnownTemplateValue(
  current: string,
  baseValue: string,
  localizedValues: Array<string | undefined>,
): boolean {
  return current === baseValue || localizedValues.some((value) => !!value && current === value)
}

export function localizeEmployeeDisplay(
  templateId: string | null | undefined,
  fallback: Pick<EmployeeTemplate, 'name' | 'role' | 'description'>,
  language?: string,
): Pick<EmployeeTemplate, 'name' | 'role' | 'description'> {
  const base = findTemplate(templateId)
  if (!base) return fallback
  const locale = normalizeLocale(language)
  const display = BUILTIN_TEMPLATE_I18N[base.templateId]?.[locale]
  if (!display) return fallback
  const allDisplays = Object.values(BUILTIN_TEMPLATE_I18N[base.templateId] ?? {})
  return {
    name: matchesKnownTemplateValue(fallback.name, base.name, allDisplays.map((item) => item?.name))
      ? (display.name ?? fallback.name)
      : fallback.name,
    role: matchesKnownTemplateValue(fallback.role, base.role, allDisplays.map((item) => item?.role))
      ? (display.role ?? fallback.role)
      : fallback.role,
    description: matchesKnownTemplateValue(
      fallback.description,
      base.description,
      allDisplays.map((item) => item?.description),
    )
      ? (display.description ?? fallback.description)
      : fallback.description,
  }
}

/**
 * Per-template-id `resource_config_kind` lookup. Hardcoded because the
 * backend `EmployeeTemplateSnapshot` doesn't carry this field today —
 * the schema-driven form (PR6) will replace it with `resourceConfigSchema`.
 *
 * Custom (`org:` / `private:`) templates default to `'none'` until they
 * ship with their own schema.
 */
const RESOURCE_CONFIG_KIND_BY_ID: Record<string, ResourceConfigKind> = {
  'builtin:xiaoyuan': 'monitoring-urls',
  'builtin:xiaofa': 'none',
  'builtin:xiaosuan': 'none',
  'builtin:xiaoxiao': 'sales-table',
  'builtin:xiaoding': 'none',
  'builtin:xiaozhao': 'none',
  'builtin:xiaozhou': 'weekly-report',
  'builtin:xiaobiao': 'none',
  'builtin:xiaogong': 'tech-support',
  'builtin:xiaoke': 'customer-support',
}

/**
 * Convert a backend `EmployeeTemplateSnapshot` (returned by
 * `employeeTemplateCatalog()`) to the frontend `EmployeeTemplate` shape
 * the wizard / cards / drawer expect.
 *
 * Field-level mapping notes:
 *   - `cron`: backend uses `""` for "no default cron"; frontend uses `null`
 *   - `defaultSkillId`: same — `""` → `null`
 *   - `systemPromptExtra`: `""` is allowed; we keep empty string as-is
 *     because the runtime concat logic tolerates it
 *   - `resourceConfigKind`: looked up by id, defaults to `'none'`
 * Builtin ids still use the local `resourceConfigKind` mapping for their
 * hand-tuned forms, but all user-facing template fields come from the
 * backend snapshot so server sync updates the visible catalog immediately.
 */
export function snapshotToTemplate(
  snap: EmployeeTemplateSnapshot,
  language?: string,
  directoryItem?: WorkplaceDirectoryItem,
  workplaceCategoryName?: string | null,
): EmployeeTemplate {
  const display = selectTemplateDisplay(snap, language)
  const directoryDisplay = directoryItem?.display
  const avatarAssetKey = display.avatarAssetKey ?? snap.avatarAssetKey ?? null
  const avatarUrl = normalizeTemplateAvatarUrl(display.avatarUrl ?? snap.avatarUrl, avatarAssetKey)
  return {
    templateId: snap.templateId,
    version: snap.version,
    avatar: directoryItem?.icon || snap.avatar,
    avatarAssetKey,
    avatarUrl,
    name: directoryDisplay?.name || display.name || snap.name,
    role: display.role ?? snap.role,
    description: directoryDisplay?.description || display.description || snap.description,
    toolWhitelist: snap.toolWhitelist,
    cron: snap.cron === '' ? null : snap.cron,
    systemPromptExtra: selectTemplatePrompt(snap, language) ?? snap.systemPromptExtra,
    badge: display.badge ?? snap.badge,
    defaultSkillId: snap.defaultSkillId === '' ? null : snap.defaultSkillId,
    requiresAttachment: snap.requiresAttachment,
    resourceConfigKind: RESOURCE_CONFIG_KIND_BY_ID[snap.templateId] ?? 'none',
    requiresDingtalk: snap.requiresDingtalk,
    requiredSkills: directoryItem?.requiredSkills,
    examples: directoryDisplay?.examples?.filter(Boolean),
    workplaceCategoryId: directoryItem?.workplaceCategoryId ?? null,
    workplaceCategoryName: workplaceCategoryName ?? null,
    resourceConfigSchema: snap.resourceConfigSchema,
  }
}
