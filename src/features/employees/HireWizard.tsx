import { useState } from 'react'
import { employeeCreate } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'

// ─── built-in templates ───────────────────────────────────────────────────────

interface EmployeeTemplate {
  templateId: string
  avatar: string
  name: string
  role: string
  description: string
  toolWhitelist: string[]
  cron: string | null
  systemPromptExtra: string
  badge: string
}

const BUILTIN_TEMPLATES: EmployeeTemplate[] = [
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
  },
]

// ─── wizard ───────────────────────────────────────────────────────────────────

interface HireWizardProps {
  open: boolean
  onClose: () => void
  onHired: () => Promise<void>
}

export function HireWizard({ open, onClose, onHired }: HireWizardProps) {
  const [step, setStep] = useState<1 | 2>(1)
  const [selected, setSelected] = useState<EmployeeTemplate | null>(null)
  const [name, setName] = useState('')
  const [enableCron, setEnableCron] = useState(true)
  const [cron, setCron] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  function handleClose() {
    setStep(1)
    setSelected(null)
    setName('')
    setEnableCron(true)
    setCron('')
    setError(null)
    onClose()
  }

  function handleSelectTemplate(t: EmployeeTemplate) {
    setSelected(t)
    setName(t.name)
    setEnableCron(!!t.cron)
    setCron(t.cron ?? '')
    setStep(2)
  }

  async function handleHire() {
    if (!selected || !name.trim()) return
    setBusy(true)
    setError(null)
    try {
      await employeeCreate({
        name: name.trim(),
        role: selected.role,
        description: selected.description,
        avatar: selected.avatar,
        templateId: selected.templateId,
        toolWhitelist: selected.toolWhitelist,
        cron: enableCron && cron.trim() ? cron.trim() : undefined,
        timezone: 'Asia/Shanghai',
        enabled: true,
        systemPromptExtra: selected.systemPromptExtra,
      })
      await onHired()
      handleClose()
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) handleClose() }}>
      <DialogContent className="max-w-2xl p-0">
        <DialogHeader className="border-b border-border px-6 py-4">
          <div className="flex items-center gap-3">
            <DialogTitle className="text-base">
              {step === 1 ? '选择员工模板' : '配置员工'}
            </DialogTitle>
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span className={step === 1 ? 'text-foreground font-medium' : ''}>1 选模板</span>
              <span>→</span>
              <span className={step === 2 ? 'text-foreground font-medium' : ''}>2 配置</span>
            </div>
          </div>
        </DialogHeader>

        {/* Step 1: template grid */}
        {step === 1 && (
          <div className="grid grid-cols-2 gap-3 p-6 sm:grid-cols-3">
            {BUILTIN_TEMPLATES.map((t) => (
              <button
                key={t.templateId}
                type="button"
                onClick={() => handleSelectTemplate(t)}
                className="flex flex-col gap-2 rounded-xl border border-border bg-card p-4 text-left transition-all hover:border-border/70 hover:shadow-sm"
              >
                <div className="flex items-center justify-between">
                  <span className="text-2xl">{t.avatar}</span>
                  <span className="rounded-full bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
                    {t.badge}
                  </span>
                </div>
                <div>
                  <p className="text-sm font-semibold text-foreground">{t.name}</p>
                  <p className="text-xs text-muted-foreground">{t.role}</p>
                </div>
                <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                  {t.description}
                </p>
              </button>
            ))}
          </div>
        )}

        {/* Step 2: configure */}
        {step === 2 && selected && (
          <div className="flex flex-col gap-5 p-6">
            {/* Preview */}
            <div className="flex items-center gap-3 rounded-xl bg-accent/40 p-3">
              <span className="text-3xl">{selected.avatar}</span>
              <div>
                <p className="text-sm font-medium text-foreground">{selected.role}</p>
                <p className="text-xs text-muted-foreground line-clamp-2">{selected.description}</p>
              </div>
            </div>

            {/* Name */}
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-muted-foreground">员工名字</label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={selected.name}
                maxLength={20}
              />
            </div>

            {/* Cron */}
            {selected.cron && (
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-medium text-muted-foreground">定时触发</label>
                  <button
                    type="button"
                    onClick={() => setEnableCron((v) => !v)}
                    className={`rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors ${
                      enableCron
                        ? 'bg-green-100 text-green-700'
                        : 'bg-muted text-muted-foreground'
                    }`}
                  >
                    {enableCron ? '已启用' : '已关闭'}
                  </button>
                </div>
                {enableCron && (
                  <Input
                    value={cron}
                    onChange={(e) => setCron(e.target.value)}
                    placeholder="30 9 * * 1  （5 字段 cron）"
                    className="font-mono text-sm"
                  />
                )}
              </div>
            )}

            {error && (
              <p className="rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>
            )}

            {/* Actions */}
            <div className="flex items-center justify-between pt-1">
              <Button variant="ghost" onClick={() => setStep(1)} disabled={busy}>
                ← 返回
              </Button>
              <Button onClick={handleHire} disabled={busy || !name.trim()}>
                {busy ? '雇佣中…' : '✅ 确认雇佣'}
              </Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
