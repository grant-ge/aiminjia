import { UserRoundPlus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '@/components/ui/dialog'
import type { EmployeeRecord } from '@/lib/tauri'
import type { EmployeeTemplate } from './templates'
import { requiredSkillNames } from './employeeCatalog'
import { getEmployeeVisual } from './employeeVisual'
import { Button } from '@/components/ui/button'

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
      <DialogContent className="max-h-[min(86vh,calc(100vh-32px))] w-[calc(100vw-32px)] max-w-[680px] gap-0 overflow-hidden rounded-md border-border/70 p-0 shadow-[0_14px_48px_rgba(0,0,0,0.13)]" data-aijia-employee-detail>
        <DialogTitle className="sr-only">{visual.name}</DialogTitle>
        <DialogDescription className="sr-only">
          {template.description}
        </DialogDescription>
        <div className="flex max-h-[min(86vh,calc(100vh-32px))] flex-col overflow-hidden">
          <div data-aijia-employee-detail-chrome className="border-b border-border/70 bg-card px-5 py-5">
            <div className="flex items-start gap-4 pr-10">
              <div
                data-aijia-employee-detail-avatar
                className={`flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-md ${visual.accent}`}
              >
                {visual.avatarUrl ? (
                  <img src={visual.avatarUrl} alt="" className="h-full w-full object-cover" />
                ) : (
                  <span className="text-2xl font-semibold leading-none">{visual.avatarText}</span>
                )}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="truncate text-[20px] font-bold leading-6 text-foreground">{visual.name}</h2>
                  <span className="rounded-md bg-[rgba(var(--primary-rgb),0.10)] px-2 py-0.5 text-xs font-medium text-primary">
                    {visual.title}
                  </span>
                </div>
                <p className="mt-1.5 line-clamp-2 text-xs leading-5 text-muted-foreground">{template.description}</p>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {strengths.slice(0, 3).map((strength) => (
                    <span key={strength} className="rounded-[2px] bg-muted px-2 py-0.5 text-2xs font-medium text-muted-foreground">
                      {strength}
                    </span>
                  ))}
                </div>
              </div>
            </div>
          </div>

          <div className="min-h-0 overflow-auto px-5 py-4">
            <section>
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('employeesPage.detail.intro')}</h3>
              <p className="mt-1.5 text-xs leading-5 text-foreground">{template.description}</p>
            </section>

            <section className="mt-4">
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('employeesPage.detail.overview')}</h3>
              <div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">
                {meta.map((item) => (
                  <div key={item.label} className="rounded-md border border-border/70 bg-muted/20 px-2.5 py-2">
                    <div className="text-xs leading-4 text-muted-foreground">{item.label}</div>
                    <div className="mt-0.5 truncate text-xs font-medium leading-4 text-foreground">{item.value}</div>
                  </div>
                ))}
              </div>
            </section>

            <section className="mt-4">
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('employeesPage.detail.strengths')}</h3>
              <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-3">
                {strengths.slice(0, 3).map((strength, index) => (
                  <div key={strength} className="rounded-md border border-border/70 bg-card px-3 py-2.5 shadow-[0_1px_2px_rgba(0,0,0,0.03)]">
                    <div data-aijia-employee-strength-row className="flex min-h-5 items-center gap-2.5">
                      <span
                        data-aijia-employee-strength-index
                        className="shrink-0 font-mono text-2xs font-semibold leading-5 text-muted-foreground/70"
                        aria-hidden="true"
                      >
                        {String(index + 1).padStart(2, '0')}
                      </span>
                      <span className="text-xs leading-5 text-foreground">{strength}</span>
                    </div>
                  </div>
                ))}
              </div>
            </section>

            <section className="mt-4">
              <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('employeesPage.detail.examples')}</h3>
              <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-2">
                {visual.examples.slice(0, 4).map((example) => (
                  <div
                    key={example}
                    className="rounded-md border border-border/70 bg-card px-3 py-2.5 text-xs leading-5 text-foreground shadow-[0_1px_2px_rgba(0,0,0,0.025)]"
                  >
                    {example}
                  </div>
                ))}
              </div>
            </section>

            {skills.length > 0 && (
              <section className="mt-4">
                <h3 className="text-xs font-semibold leading-4 text-muted-foreground">{t('employeesPage.detail.skillSet')}</h3>
                <div className="mt-2 flex flex-wrap gap-1.5">
                  {skills.map((skill) => (
                    <span key={skill} className="rounded-[2px] bg-muted px-2 py-0.5 text-2xs font-medium text-muted-foreground">
                      {skill}
                    </span>
                  ))}
                </div>
              </section>
            )}
          </div>
          <div className="flex shrink-0 justify-end border-t border-border/70 bg-card px-5 py-3">
            <Button
              type="button"
              loading={busy}
              icon={<UserRoundPlus />}
              onClick={() => onStart(template)}
            >
              {actionLabel}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
