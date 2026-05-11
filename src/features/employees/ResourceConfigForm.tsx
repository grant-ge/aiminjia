import { useTranslation } from 'react-i18next'

import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { MonitoringUrlsForm } from './forms/MonitoringUrlsForm'
import { SalesTableConfigForm } from './forms/SalesTableConfigForm'
import { WeeklyReportConfigForm } from './forms/WeeklyReportConfigForm'
import { TechSupportConfigForm } from './forms/TechSupportConfigForm'
import { CustomerSupportConfigForm } from './forms/CustomerSupportConfigForm'
import type { ResourceConfigKind } from './templates'

interface ResourceConfigFormProps {
  open: boolean
  kind: ResourceConfigKind
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

const TITLE_KEY: Record<Exclude<ResourceConfigKind, 'none'>, string> = {
  'monitoring-urls': 'employee.config.monitoringUrls.title',
  'sales-table': 'employee.config.salesTable.title',
  'weekly-report': 'employee.config.weeklyReport.title',
  'tech-support': 'employee.config.techSupport.title',
  'customer-support': 'employee.config.customerSupport.title',
}

export function ResourceConfigForm({ open, kind, initial, onSubmit, onCancel }: ResourceConfigFormProps) {
  const { t } = useTranslation()
  if (kind === 'none') return null

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onCancel() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-base">{t(TITLE_KEY[kind])}</DialogTitle>
        </DialogHeader>
        {kind === 'monitoring-urls' && (
          <MonitoringUrlsForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
        {kind === 'sales-table' && (
          <SalesTableConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
        {kind === 'weekly-report' && (
          <WeeklyReportConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
        {kind === 'tech-support' && (
          <TechSupportConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
        {kind === 'customer-support' && (
          <CustomerSupportConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
      </DialogContent>
    </Dialog>
  )
}
