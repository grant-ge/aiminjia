import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw } from 'lucide-react'
import {
  employeeCreate,
  employeeIndexKnowledgeAsync,
  type PendingKnowledgeSource,
} from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { EmployeeTemplate } from './templates'
import { loadEmployeeTemplateCatalog, requiredSkillNames } from './employeeCatalog'
import { MonitoringUrlsForm } from './forms/MonitoringUrlsForm'
import { SalesTableConfigForm } from './forms/SalesTableConfigForm'
import { WeeklyReportConfigForm } from './forms/WeeklyReportConfigForm'
import { TechSupportConfigForm } from './forms/TechSupportConfigForm'
import { CustomerSupportConfigForm } from './forms/CustomerSupportConfigForm'
import { SchemaForm, type JsonSchema } from './forms/SchemaForm'

/**
 * True when the template ships a non-empty JSON Schema for instance
 * config. PR6 (2026-05-10): custom org / private templates use a
 * schema-driven form instead of the closed `ResourceConfigKind` enum.
 * BUILTIN_TEMPLATES leave the schema empty and keep their hand-tuned forms.
 */
function hasSchemaForm(template: EmployeeTemplate): boolean {
  const schema = template.resourceConfigSchema
  return (
    !!schema &&
    typeof schema === 'object' &&
    Object.keys((schema as Record<string, unknown>).properties ?? {}).length > 0
  )
}

// ─── wizard ───────────────────────────────────────────────────────────────────

interface HireWizardProps {
  open: boolean
  onClose: () => void
  onHired: () => Promise<void>
}

export function HireWizard({ open, onClose, onHired }: HireWizardProps) {
  const { t, i18n } = useTranslation()
  const [step, setStep] = useState<1 | 2 | 3>(1)
  const [selected, setSelected] = useState<EmployeeTemplate | null>(null)
  // Catalog 来源（按时间顺序）：
  // 1. 弹窗打开 → 触发 workplace_directory_catalog 拉目录并预热 snapshot cache
  // 2. 读 employee_template_catalog —— 后端读 cache 目录
  // 3. 目录不可用时回退 employee_template_refresh + 本地 cache
  // 4. cache 仍为空 → UI 显示"加载/重试"，不再 fallback 到硬编码 BUILTIN_TEMPLATES
  const [catalog, setCatalog] = useState<EmployeeTemplate[]>([])
  const [catalogLoading, setCatalogLoading] = useState(false)
  const [catalogLoadError, setCatalogLoadError] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [enableCron, setEnableCron] = useState(true)
  const [cron, setCron] = useState('')
  const [resourceConfig, setResourceConfig] = useState<Record<string, unknown>>({})
  const [busy, setBusy] = useState(false)
  const [syncingTemplates, setSyncingTemplates] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 弹窗打开时：先走 workplace directory，失败再回退老模板缓存。
  // 两条路径都拿不到内容时显式 surface 错误，让用户点"再次同步"重试。
  useEffect(() => {
    if (!open) return
    let cancelled = false
    void (async () => {
      setCatalogLoading(true)
      setCatalogLoadError(null)
      try {
        const result = await loadEmployeeTemplateCatalog(i18n.language)
        if (cancelled) return
        setCatalog(result.catalog)
        setCatalogLoadError(
          result.error ? (result.error instanceof Error ? result.error.message : String(result.error)) : null,
        )
      } catch (e) {
        if (cancelled) return
        console.error('[HireWizard] employee_template_catalog failed:', e)
        setCatalog([])
        setCatalogLoadError(e instanceof Error ? e.message : String(e))
      } finally {
        if (!cancelled) setCatalogLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [open, i18n.language])

  function handleClose() {
    setStep(1)
    setSelected(null)
    setName('')
    setEnableCron(true)
    setCron('')
    setResourceConfig({})
    setError(null)
    onClose()
  }

  function handleSelectTemplate(t: EmployeeTemplate) {
    setSelected(t)
    setName(t.name)
    setEnableCron(!!t.cron)
    setCron(t.cron ?? '')
    setResourceConfig({})
    setStep(2)
  }

  async function reloadCatalog() {
    const result = await loadEmployeeTemplateCatalog(i18n.language)
    setCatalog(result.catalog)
    setCatalogLoadError(
      result.error ? (result.error instanceof Error ? result.error.message : String(result.error)) : null,
    )
  }

  async function handleSyncTemplates() {
    if (syncingTemplates) return
    setSyncingTemplates(true)
    setError(null)
    try {
      await reloadCatalog()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSyncingTemplates(false)
    }
  }

  async function hireWithConfig(cfg: Record<string, unknown>) {
    if (!selected || !name.trim()) return
    setBusy(true)
    setError(null)
    try {
      const created = await employeeCreate({
        name: name.trim(),
        role: selected.role,
        description: selected.description,
        avatar: selected.avatar,
        templateId: selected.templateId,
        toolWhitelist: selected.toolWhitelist,
        cron: enableCron && cron.trim() ? cron.trim() : undefined,
        timezone: 'Asia/Shanghai',
        lifecycle: 'active',
        cronEnabled: enableCron,
        systemPromptExtra: selected.systemPromptExtra,
        defaultSkillId: selected.defaultSkillId ?? undefined,
        resourceConfig: cfg,
      })
      const rawSources = (cfg.knowledgeSources as Array<Record<string, unknown>> | undefined) ?? []
      const pending: PendingKnowledgeSource[] = rawSources.flatMap((s) => {
        if (typeof s.path !== 'string' || typeof s.originalName !== 'string') return []
        const status = s.status
        if (status && status !== 'pending' && status !== 'failed') return []
        return [{ path: s.path, originalName: s.originalName, size: typeof s.size === 'number' ? s.size : 0 }]
      })
      if (pending.length > 0) {
        void employeeIndexKnowledgeAsync(created.id, pending)
      }
      await onHired()
      handleClose()
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  function handleStep2Next() {
    if (!selected) return
    if (selected.resourceConfigKind === 'none' && !hasSchemaForm(selected)) {
      void hireWithConfig(resourceConfig)
    } else {
      setStep(3)
    }
  }

  async function handleResourceSubmit(next: Record<string, unknown>) {
    setResourceConfig(next)
    await hireWithConfig(next)
  }

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) handleClose() }}>
      <DialogContent data-aijia-hire-wizard data-aijia-hire-step={step} className="max-w-2xl p-0">
        <DialogDescription className="sr-only">
          {t('employee.config.wizard.description', '选择数字员工模板并完成必要配置。')}
        </DialogDescription>
        <DialogHeader className="border-b border-border px-6 py-4">
          <div className="flex items-center justify-between gap-3">
            <div className="flex min-w-0 items-center gap-3">
              <DialogTitle className="text-base">
                {step === 1
                  ? t('employee.config.wizard.titleStep1')
                  : step === 2
                    ? t('employee.config.wizard.titleStep2')
                    : t('employee.config.wizard.titleStep3')}
              </DialogTitle>
              <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <span className={step === 1 ? 'text-foreground font-medium' : ''}>{t('employee.config.wizard.stepLabel1')}</span>
                <span>→</span>
                <span className={step === 2 ? 'text-foreground font-medium' : ''}>{t('employee.config.wizard.stepLabel2')}</span>
                {selected && (selected.resourceConfigKind !== 'none' || hasSchemaForm(selected)) && (
                  <>
                    <span>→</span>
                    <span className={step === 3 ? 'text-foreground font-medium' : ''}>{t('employee.config.wizard.stepLabel3')}</span>
                  </>
                )}
              </div>
            </div>
            {step === 1 ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 gap-1.5 px-2 text-xs"
                disabled={syncingTemplates}
                onClick={() => void handleSyncTemplates()}
              >
                <RefreshCw className={`h-3 w-3 ${syncingTemplates ? 'animate-spin' : ''}`} />
                {syncingTemplates ? t('employeesPage.syncing') : t('employeesPage.syncServer')}
              </Button>
            ) : null}
          </div>
        </DialogHeader>

        {/* Step 1: template grid */}
        {step === 1 && (
          <div className="grid grid-cols-2 gap-3 p-6 sm:grid-cols-3">
            {error && (
              <p className="col-span-full rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>
            )}
            {catalogLoading && catalog.length === 0 && (
              <div
                className="col-span-full flex flex-col items-center gap-2 rounded-lg border border-dashed border-border bg-muted/30 py-12 text-sm text-muted-foreground"
                data-aijia-hire-template-loading
              >
                <RefreshCw className="h-4 w-4 animate-spin" />
                <span>{t('employee.config.wizard.catalogLoading', '正在从服务端拉取员工模板…')}</span>
              </div>
            )}
            {!catalogLoading && catalog.length === 0 && (
              <div
                className="col-span-full flex flex-col items-center gap-3 rounded-lg border border-dashed border-border bg-muted/30 px-4 py-12 text-center text-sm text-muted-foreground"
                data-aijia-hire-template-empty
              >
                <p>
                  {catalogLoadError
                    ? t(
                        'employee.config.wizard.catalogLoadError',
                        '没拉到员工模板：{{err}}。网络恢复后点"再次同步"重试。',
                        { err: catalogLoadError },
                      )
                    : t(
                        'employee.config.wizard.catalogEmpty',
                        '本地还没有员工模板缓存。请确认网络后点"再次同步"。',
                      )}
                </p>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={handleSyncTemplates}
                  disabled={syncingTemplates}
                  data-aijia-hire-action="catalog-retry"
                >
                  <RefreshCw className={syncingTemplates ? 'mr-1.5 h-3.5 w-3.5 animate-spin' : 'mr-1.5 h-3.5 w-3.5'} />
                  {syncingTemplates ? t('employee.config.wizard.syncing', '同步中…') : t('employee.config.wizard.syncRetry', '再次同步')}
                </Button>
              </div>
            )}
            {catalog.map((template) => {
              const skills = requiredSkillNames(template)
              return (
                <button
                  key={template.templateId}
                  type="button"
                  data-aijia-hire-template
                  data-aijia-hire-template-id={template.templateId}
                  data-aijia-hire-template-name={template.name}
                  onClick={() => handleSelectTemplate(template)}
                  className="flex flex-col gap-2 rounded-xl border border-border bg-card p-4 text-left transition-all hover:border-border/70 hover:shadow-sm"
                >
                  <div className="flex items-start justify-between gap-2">
                    <span className="text-2xl">{template.avatar}</span>
                    <div className="flex min-w-0 flex-wrap justify-end gap-1">
                      {template.workplaceCategoryName && (
                        <span className="max-w-[96px] truncate rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">
                          {template.workplaceCategoryName}
                        </span>
                      )}
                      {template.version && (
                        <span className="rounded-full bg-secondary px-1.5 py-0.5 text-[10px] text-muted-foreground">
                          v{template.version}
                        </span>
                      )}
                      <span className="max-w-[96px] truncate rounded-full bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
                        {template.badge}
                      </span>
                    </div>
                  </div>
                  <div>
                    <p className="text-sm font-semibold text-foreground">{template.name}</p>
                    <p className="text-xs text-muted-foreground">{template.role}</p>
                  </div>
                  <p className="line-clamp-2 text-xs leading-relaxed text-muted-foreground">
                    {template.description}
                  </p>
                  {skills.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {skills.slice(0, 3).map((skill) => (
                        <span
                          key={skill}
                          className="max-w-full truncate rounded-full bg-accent px-1.5 py-0.5 text-[10px] text-accent-foreground"
                        >
                          {skill}
                        </span>
                      ))}
                    </div>
                  )}
                </button>
              )
            })}
          </div>
        )}

        {/* Step 2: configure */}
        {step === 2 && selected && (
          <div className="flex flex-col gap-5 p-6">
            {/* Preview */}
            <div className="flex items-center gap-3 rounded-xl bg-accent/40 p-3">
              <span className="text-3xl">{selected.avatar}</span>
              <div className="min-w-0">
                <p className="text-sm font-medium text-foreground">{selected.role}</p>
                <p className="text-xs text-muted-foreground line-clamp-2">{selected.description}</p>
                {requiredSkillNames(selected).length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1">
                    {requiredSkillNames(selected).slice(0, 4).map((skill) => (
                      <span
                        key={skill}
                        className="max-w-full truncate rounded-full bg-background/80 px-1.5 py-0.5 text-[10px] text-muted-foreground"
                      >
                        {skill}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* Name */}
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-muted-foreground">{t('employee.config.wizard.nameLabel')}</label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                data-aijia-hire-field="name"
                placeholder={selected.name}
                maxLength={20}
              />
            </div>

            {/* Cron */}
            {selected.cron && (
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-medium text-muted-foreground">{t('employee.config.wizard.cronLabel')}</label>
                  <button
                    type="button"
                    onClick={() => setEnableCron((v) => !v)}
                    className={`rounded-full px-2.5 py-0.5 text-xs font-medium transition-colors ${
                      enableCron
                        ? 'bg-green-100 text-green-700'
                        : 'bg-muted text-muted-foreground'
                    }`}
                  >
                    {enableCron ? t('employee.config.wizard.cronEnabled') : t('employee.config.wizard.cronDisabled')}
                  </button>
                </div>
                {enableCron && (
                  <Input
                    value={cron}
                    onChange={(e) => setCron(e.target.value)}
                    data-aijia-hire-field="cron"
                    placeholder={t('employee.config.wizard.cronPlaceholder')}
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
              <Button variant="ghost" data-aijia-hire-action="prev" onClick={() => setStep(1)} disabled={busy}>
                {t('employee.config.wizard.back')}
              </Button>
              <Button data-aijia-hire-action={selected.resourceConfigKind === 'none' && !hasSchemaForm(selected) ? 'save' : 'next'} onClick={handleStep2Next} disabled={busy || !name.trim()}>
                {busy
                  ? t('employee.config.wizard.hiring')
                  : selected.resourceConfigKind === 'none' && !hasSchemaForm(selected)
                    ? t('employee.config.wizard.confirmHire')
                    : t('employee.config.wizard.next')}
              </Button>
            </div>
          </div>
        )}

        {/* Step 3: resource config (inline — HireWizard already uses a Dialog) */}
        {step === 3 && selected && (
          <div className="p-6">
            {selected.resourceConfigKind === 'monitoring-urls' && (
              <MonitoringUrlsForm
                initial={resourceConfig}
                onSubmit={handleResourceSubmit}
                onCancel={() => setStep(2)}
              />
            )}
            {selected.resourceConfigKind === 'sales-table' && (
              <SalesTableConfigForm
                initial={resourceConfig}
                onSubmit={handleResourceSubmit}
                onCancel={() => setStep(2)}
              />
            )}
            {selected.resourceConfigKind === 'weekly-report' && (
              <WeeklyReportConfigForm
                initial={resourceConfig}
                onSubmit={handleResourceSubmit}
                onCancel={() => setStep(2)}
              />
            )}
            {selected.resourceConfigKind === 'tech-support' && (
              <TechSupportConfigForm
                initial={resourceConfig}
                onSubmit={handleResourceSubmit}
                onCancel={() => setStep(2)}
              />
            )}
            {selected.resourceConfigKind === 'customer-support' && (
              <CustomerSupportConfigForm
                initial={resourceConfig}
                onSubmit={handleResourceSubmit}
                onCancel={() => setStep(2)}
              />
            )}
            {selected.resourceConfigKind === 'none' && hasSchemaForm(selected) && (
              <SchemaForm
                schema={selected.resourceConfigSchema as JsonSchema}
                initial={resourceConfig}
                onSubmit={handleResourceSubmit}
                onCancel={() => setStep(2)}
              />
            )}
            {error && (
              <p className="mt-3 rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
