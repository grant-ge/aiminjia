// Single source of truth for the 5 built-in employee templates.
// Read both by HireWizard (during 雇佣) and EmployeeDrawer (for trigger prechecks).

export type ResourceConfigKind = 'monitoring-urls' | 'sales-table' | 'none'

export interface RequiresAttachmentSpec {
  /** Comma-separated extension list passed to the file picker, e.g. `.pdf,.docx`. */
  accept: string
  min: number
  max: number
}

export interface EmployeeTemplate {
  templateId: string
  avatar: string
  name: string
  role: string
  description: string
  toolWhitelist: string[]
  cron: string | null
  systemPromptExtra: string
  badge: string
  /** When set, dispatch_employee_run prepends "请第一步调用 load_skill('<id>')" to the user message. */
  defaultSkillId: string | null
  /** When set, EmployeeDrawer.handleTrigger opens a file picker before calling employee_trigger. */
  requiresAttachment: RequiresAttachmentSpec | null
  /** Drives which ResourceConfigForm subcomponent is shown in HireWizard step 3 + the Drawer ⚙️ button. */
  resourceConfigKind: ResourceConfigKind
  /** True when an employee with this templateId requires `dingtalk_status().connected === true` before dispatch. */
  requiresDingtalk: boolean
}

export const BUILTIN_TEMPLATES: EmployeeTemplate[] = [
  {
    templateId: 'builtin:xiaoyuan',
    avatar: '🔍',
    name: '小研',
    role: '行业/竞品调研员',
    description: '每周汇总竞品和行业渠道的产品发布、定价、招聘、媒体报道四个维度的变化，去重后生成周报。',
    toolWhitelist: ['web_search', 'browse_and_extract', 'browse_navigate', 'extract_table_data', 'read_page_content', 'memory_save', 'memory_search', 'load_skill', 'generate_report'],
    cron: '0 9 * * 1',
    systemPromptExtra: '你是一名专注于竞品与行业调研的分析师。请聚焦于事实与信号，不做战略评估。',
    badge: '🟢 开箱即用',
    defaultSkillId: 'competitive-intelligence',
    requiresAttachment: null,
    resourceConfigKind: 'monitoring-urls',
    requiresDingtalk: false,
  },
  {
    templateId: 'builtin:xiaofa',
    avatar: '⚖️',
    name: '小法',
    role: '合同审阅员',
    description: '按 10 大风险条款扫描 PDF/DOCX 合同，输出风险标注与改写建议。',
    toolWhitelist: ['load_file', 'read_file', 'grep_content', 'edit_file', 'load_skill', 'generate_report'],
    cron: null,
    systemPromptExtra: '你是一名合同风险审查员。请严格按条款逐一扫描，不替代律师意见。',
    badge: '🟢 开箱即用',
    defaultSkillId: 'contract-review',
    requiresAttachment: { accept: '.pdf,.docx', min: 1, max: 5 },
    resourceConfigKind: 'none',
    requiresDingtalk: false,
  },
  {
    templateId: 'builtin:xiaosuan',
    avatar: '📊',
    name: '小算',
    role: '数据分析员',
    description: '自动 EDA + 异常检测 + 图表 + 假设检验 + 报告/PPT，支持 Excel/CSV 数据。',
    toolWhitelist: ['load_file', 'browse_data', 'execute_python', 'generate_chart', 'detect_anomalies', 'hypothesis_test', 'analysis_note', 'generate_report', 'generate_slides', 'export_data'],
    cron: null,
    systemPromptExtra: '你是一名数据分析师。使用 Python (pandas/scipy/matplotlib) 处理数据，产出可视化报告。',
    badge: '🟢 开箱即用',
    defaultSkillId: null,
    requiresAttachment: { accept: '.xlsx,.xls,.csv,.json', min: 1, max: 3 },
    resourceConfigKind: 'none',
    requiresDingtalk: false,
  },
  {
    templateId: 'builtin:xiaoxiao',
    avatar: '💼',
    name: '小销',
    role: '客户跟进员',
    description: '每个工作日早上读钉钉 AI 表格中的在谈客户，按优先级判定今天该跟进谁，口述结果后反向同步表格。',
    toolWhitelist: ['dingtalk_list_bases', 'dingtalk_schema', 'dingtalk_query_records', 'dingtalk_update_record', 'dingtalk_search_chat', 'web_search', 'memory_save', 'memory_search', 'generate_report'],
    cron: '30 8 * * 1-5',
    systemPromptExtra: '你是一名客户关系跟进员。写操作必须经用户明确确认后再执行。',
    badge: '🟠 需配置数据源',
    defaultSkillId: 'sales-followup-rules',
    requiresAttachment: null,
    resourceConfigKind: 'sales-table',
    requiresDingtalk: true,
  },
  {
    templateId: 'builtin:xiaoding',
    avatar: '📌',
    name: '小钉',
    role: '钉办助理',
    description: '每天早晨汇总日程/待办/群聊重点，按需找空闲时段约会议、用户确认后发消息。',
    toolWhitelist: ['dingtalk_list_events', 'dingtalk_create_event', 'dingtalk_free_busy', 'dingtalk_list_todos', 'dingtalk_create_todo', 'dingtalk_complete_todo', 'dingtalk_search_chat', 'dingtalk_send_message', 'dingtalk_search_contacts', 'generate_report'],
    cron: '0 9 * * 1-5',
    systemPromptExtra: '你是一名钉钉日程助理。发消息和创建日程必须经用户明确确认。',
    badge: '🟡 需授权钉钉',
    defaultSkillId: null,
    requiresAttachment: null,
    resourceConfigKind: 'none',
    requiresDingtalk: true,
  },
]

/** Look up the template that produced an employee, by `EmployeeRecord.templateId`. */
export function findTemplate(templateId: string | null | undefined): EmployeeTemplate | null {
  if (!templateId) return null
  return BUILTIN_TEMPLATES.find((t) => t.templateId === templateId) ?? null
}
