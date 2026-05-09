import { useState } from 'react'
import { employeeCreate, employeeIndexKnowledgeAsync, type PendingKnowledgeSource } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { BUILTIN_TEMPLATES, type EmployeeTemplate } from './templates'
import { MonitoringUrlsForm } from './forms/MonitoringUrlsForm'
import { SalesTableConfigForm } from './forms/SalesTableConfigForm'
import { WeeklyReportConfigForm } from './forms/WeeklyReportConfigForm'
import { TechSupportConfigForm } from './forms/TechSupportConfigForm'
import { CustomerSupportConfigForm } from './forms/CustomerSupportConfigForm'

// ─── wizard ───────────────────────────────────────────────────────────────────

interface HireWizardProps {
  open: boolean
  onClose: () => void
  onHired: () => Promise<void>
}

export function HireWizard({ open, onClose, onHired }: HireWizardProps) {
  const [step, setStep] = useState<1 | 2 | 3>(1)
  const [selected, setSelected] = useState<EmployeeTemplate | null>(null)
  const [name, setName] = useState('')
  const [enableCron, setEnableCron] = useState(true)
  const [cron, setCron] = useState('')
  const [resourceConfig, setResourceConfig] = useState<Record<string, unknown>>({})
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

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
    if (selected.resourceConfigKind === 'none') {
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
      <DialogContent className="max-w-2xl p-0">
        <DialogHeader className="border-b border-border px-6 py-4">
          <div className="flex items-center gap-3">
            <DialogTitle className="text-base">
              {step === 1 ? '选择员工模板' : step === 2 ? '配置员工' : '配置资源'}
            </DialogTitle>
            <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span className={step === 1 ? 'text-foreground font-medium' : ''}>1 选模板</span>
              <span>→</span>
              <span className={step === 2 ? 'text-foreground font-medium' : ''}>2 配置</span>
              {selected?.resourceConfigKind !== 'none' && (
                <>
                  <span>→</span>
                  <span className={step === 3 ? 'text-foreground font-medium' : ''}>3 资源</span>
                </>
              )}
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
              <Button onClick={handleStep2Next} disabled={busy || !name.trim()}>
                {busy ? '雇佣中…' : selected.resourceConfigKind === 'none' ? '✅ 确认雇佣' : '下一步 →'}
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
            {error && (
              <p className="mt-3 rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
