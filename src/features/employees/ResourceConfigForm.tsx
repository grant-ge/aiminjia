import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { MonitoringUrlsForm } from './forms/MonitoringUrlsForm'
import { SalesTableConfigForm } from './forms/SalesTableConfigForm'
import type { ResourceConfigKind } from './templates'

interface ResourceConfigFormProps {
  open: boolean
  kind: ResourceConfigKind
  initial: Record<string, unknown>
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

export function ResourceConfigForm({ open, kind, initial, onSubmit, onCancel }: ResourceConfigFormProps) {
  if (kind === 'none') return null

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onCancel() }}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-base">
            {kind === 'monitoring-urls' ? '配置监测对象' : '配置数据源'}
          </DialogTitle>
        </DialogHeader>
        {kind === 'monitoring-urls' && (
          <MonitoringUrlsForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
        {kind === 'sales-table' && (
          <SalesTableConfigForm initial={initial} onSubmit={onSubmit} onCancel={onCancel} />
        )}
      </DialogContent>
    </Dialog>
  )
}
