import { RefreshCw, SendHorizontal, Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import type { EmployeeRecord } from '@/lib/tauri'
import type { EmployeeTemplate } from './templates'
import { requiredSkillNames } from './employeeCatalog'
import { getEmployeeVisual } from './employeeVisual'

interface EmployeeTemplateDetailDialogProps {
  template: EmployeeTemplate | null
  existingEmployee: EmployeeRecord | null
  runningConversationId: string | null
  open: boolean
  busy: boolean
  onOpenChange: (open: boolean) => void
  onStart: (template: EmployeeTemplate) => void
}

function scheduleLabel(template: EmployeeTemplate, language: string): string {
  if (!template.cron) return language.toLowerCase().startsWith('en') ? 'On demand' : '按需派活'
  return language.toLowerCase().startsWith('en') ? 'On demand + scheduled' : '按需 + 可定时'
}

export function EmployeeTemplateDetailDialog({
  template,
  existingEmployee,
  runningConversationId,
  open,
  busy,
  onOpenChange,
  onStart,
}: EmployeeTemplateDetailDialogProps) {
  const { t, i18n } = useTranslation()
  if (!template) return null

  const visual = getEmployeeVisual(template)
  const skills = requiredSkillNames(template)
  const actionLabel = busy
    ? t('employeesPage.summoning')
    : runningConversationId
      ? t('employeesPage.enterRunning')
      : existingEmployee
        ? t('employeesPage.assignWork')
        : t('employeesPage.summon')
  const meta = [
    ...(template.workplaceCategoryName
      ? [{ label: t('employeesPage.detail.category'), value: template.workplaceCategoryName }]
      : []),
    { label: t('employeesPage.detail.version'), value: template.version ? `v${template.version}` : t('employeesPage.detail.latest') },
    { label: t('employeesPage.detail.trigger'), value: scheduleLabel(template, i18n.language) },
    { label: t('employeesPage.detail.skills'), value: skills.length > 0 ? String(skills.length) : t('employeesPage.detail.dialogFirst') },
  ]
  const strengths = visual.strengths.length > 0 ? visual.strengths : [
    template.role,
    t('employeesPage.detail.contextAware'),
    t('employeesPage.detail.resultOriented'),
  ]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(86vh,calc(100vh-32px))] w-[calc(100vw-32px)] max-w-[820px] overflow-hidden p-0" data-aijia-employee-detail>
        <DialogTitle className="sr-only">{visual.name}</DialogTitle>
        <DialogDescription className="sr-only">
          {template.description}
        </DialogDescription>
        <div className="flex max-h-[min(86vh,calc(100vh-32px))] flex-col overflow-hidden">
          <div className="flex items-start gap-5 border-b border-border bg-card px-6 py-5 pr-16">
            <div className={`flex h-20 w-20 shrink-0 items-center justify-center overflow-hidden rounded-lg ${visual.accent}`}>
              {visual.avatarUrl ? (
                <img src={visual.avatarUrl} alt="" className="h-full w-full object-cover" />
              ) : (
                <span className="text-2xl font-semibold leading-none">{visual.avatarText}</span>
              )}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <h2 className="truncate text-[22px] font-bold leading-7 text-foreground">{visual.name}</h2>
                <span className="rounded-[var(--radius)] bg-brand-primary-subtle px-2 py-0.5 text-xs font-medium text-primary">
                  {visual.title}
                </span>
              </div>
              <p className="mt-2 line-clamp-3 text-sm leading-6 text-muted-foreground">{template.description}</p>
            </div>
          </div>

          <div className="min-h-0 overflow-auto px-6 py-5">
            <div className="flex flex-wrap gap-x-12 gap-y-4">
              {meta.map((item) => (
                <div key={item.label} className="flex min-w-[96px] flex-col gap-1.5">
                  <span className="text-xs font-medium text-muted-foreground">{item.label}</span>
                  <span className="text-sm text-foreground">{item.value}</span>
                </div>
              ))}
            </div>

            <section className="mt-6 flex flex-col gap-3">
              <h3 className="text-sm font-semibold text-foreground">{t('employeesPage.detail.strengths')}</h3>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
                {strengths.slice(0, 3).map((strength) => (
                  <div key={strength} className="rounded-md border border-border bg-card px-3 py-3 shadow-[var(--shadow-card)]">
                    <div className="flex items-start gap-2">
                      <Sparkles className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
                      <span className="text-sm text-foreground">{strength}</span>
                    </div>
                  </div>
                ))}
              </div>
            </section>

            <section className="mt-6 flex flex-col gap-3">
              <h3 className="text-sm font-semibold text-foreground">{t('employeesPage.detail.examples')}</h3>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                {visual.examples.slice(0, 4).map((example) => (
                  <div key={example} className="rounded-md border border-border bg-muted/30 px-4 py-3 text-sm leading-6 text-foreground">
                    {example}
                  </div>
                ))}
              </div>
            </section>

            {skills.length > 0 && (
              <section className="mt-6 flex flex-col gap-3">
                <h3 className="text-sm font-semibold text-foreground">{t('employeesPage.detail.skillSet')}</h3>
                <div className="flex flex-wrap gap-2">
                  {skills.map((skill) => (
                    <span key={skill} className="rounded-full bg-accent px-2.5 py-1 text-xs text-accent-foreground">
                      {skill}
                    </span>
                  ))}
                </div>
              </section>
            )}
          </div>
          <div className="flex shrink-0 justify-end gap-2 border-t border-border bg-card px-6 py-4">
            <Button
              type="button"
              className="min-w-[128px] gap-1.5 px-5"
              disabled={busy}
              onClick={() => onStart(template)}
            >
              {busy ? <RefreshCw className="h-4 w-4 animate-spin" /> : <SendHorizontal className="h-4 w-4" />}
              {actionLabel}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
