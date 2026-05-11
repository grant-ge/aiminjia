import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  employeeCreate,
  employeeIndexKnowledgeAsync,
  employeeTemplateCatalog,
  employeeTemplateRefresh,
  type EmployeeTemplateSnapshot,
  type PendingKnowledgeSource,
} from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { BUILTIN_TEMPLATES, snapshotToTemplate, type EmployeeTemplate } from './templates'
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
  const { t } = useTranslation()
  const [step, setStep] = useState<1 | 2 | 3>(1)
  const [selected, setSelected] = useState<EmployeeTemplate | null>(null)
  // Catalog: backend (`employee_template_catalog` = bootstrap ∪ cache) when
  // available, falls back to the legacy hardcoded `BUILTIN_TEMPLATES` if
  // the IPC call fails (e.g. dev server with mismatched binary). The
  // wizard renders this list directly; we don't update it after open.
  const [catalog, setCatalog] = useState<EmployeeTemplate[]>(BUILTIN_TEMPLATES)
  const [name, setName] = useState('')
  const [enableCron, setEnableCron] = useState(true)
  const [cron, setCron] = useState('')
  const [resourceConfig, setResourceConfig] = useState<Record<string, unknown>>({})
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // When the dialog opens: best-effort refresh the local template cache
  // from lotus ops-portal, then load the merged catalog. Cache refresh
  // failures don't block the user — they just see whatever's already
  // local (bootstrap + any previously-downloaded versions).
  //
  // Why refresh on open and not on mount: the wizard is mounted with the
  // parent page and we don't want a network call on every app launch.
  // Opening the wizard is a deliberate user action where a 1-2s delay is
  // acceptable for the side effect of getting freshest content.
  useEffect(() => {
    if (!open) return
    let cancelled = false
    void (async () => {
      try {
        // Fire-and-forget refresh; if it fails we just use whatever the
        // backend already has cached + bootstrap.
        await employeeTemplateRefresh().catch((e) => {
          console.warn('[HireWizard] employee_template_refresh failed:', e)
          return 0
        })
        const snapshots: EmployeeTemplateSnapshot[] = await employeeTemplateCatalog()
        if (cancelled) return
        if (snapshots.length > 0) {
          setCatalog(snapshots.map(snapshotToTemplate))
        }
      } catch (e) {
        console.error('[HireWizard] employee_template_catalog failed, using BUILTIN_TEMPLATES:', e)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [open])

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
              {selected?.resourceConfigKind !== 'none' && (
                <>
                  <span>→</span>
                  <span className={step === 3 ? 'text-foreground font-medium' : ''}>{t('employee.config.wizard.stepLabel3')}</span>
                </>
              )}
            </div>
          </div>
        </DialogHeader>

        {/* Step 1: template grid */}
        {step === 1 && (
          <div className="grid grid-cols-2 gap-3 p-6 sm:grid-cols-3">
            {catalog.map((t) => (
              <button
                key={t.templateId}
                type="button"
                onClick={() => handleSelectTemplate(t)}
                className="flex flex-col gap-2 rounded-xl border border-border bg-card p-4 text-left transition-all hover:border-border/70 hover:shadow-sm"
              >
                <div className="flex items-center justify-between">
                  <span className="text-2xl">{t.avatar}</span>
                  <span className="rounded-full bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
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
              <label className="text-xs font-medium text-muted-foreground">{t('employee.config.wizard.nameLabel')}</label>
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
              <Button variant="ghost" onClick={() => setStep(1)} disabled={busy}>
                {t('employee.config.wizard.back')}
              </Button>
              <Button onClick={handleStep2Next} disabled={busy || !name.trim()}>
                {busy
                  ? t('employee.config.wizard.hiring')
                  : selected.resourceConfigKind === 'none'
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
            {error && (
              <p className="mt-3 rounded-lg bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
