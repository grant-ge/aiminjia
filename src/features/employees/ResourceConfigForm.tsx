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

function titleFor(kind: ResourceConfigKind): string {
  switch (kind) {
    case 'monitoring-urls':
      return '配置监测对象'
    case 'sales-table':
      return '配置数据源'
    case 'weekly-report':
      return '配置周报偏好'
    case 'tech-support':
      return '配置技术支持'
    case 'customer-support':
      return '配置客服支持'
    case 'none':
      return ''
  }
}

export function ResourceConfigForm({ open, kind, initial, onSubmit, onCancel }: ResourceConfigFormProps) {
  if (kind === 'none') return null

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onCancel() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-base">{titleFor(kind)}</DialogTitle>
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
