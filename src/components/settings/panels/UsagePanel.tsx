/**
 * @designSource design.pen#mbeKY planCard + BtLe0 quotaSec + SAOik detailSec
 */
import { useTranslation } from 'react-i18next'

interface QuotaItem { label: string; used: number; total: number }
interface DetailRow { label: string; value: string }

interface UsagePanelProps {
  planName: string
  planRenewLabel: string
  quota: QuotaItem[]
  detail: DetailRow[]
}

export function UsagePanel({ planName, planRenewLabel, quota, detail }: UsagePanelProps) {
  const { t } = useTranslation()
  return (
    <>
      <div className="flex items-center justify-between border-b border-border py-4">
        <div className="flex flex-col gap-1">
          <div className="text-sm font-semibold text-foreground">{planName}</div>
          <div className="text-sm text-muted-foreground">{planRenewLabel}</div>
        </div>
      </div>
      <section className="flex flex-col gap-5">
        {quota.map((q) => {
          const pct = Math.min(100, Math.round((q.used / Math.max(1, q.total)) * 100))
          return (
            <div key={q.label} className="flex flex-col gap-2">
              <div className="flex items-center justify-between text-sm text-foreground">
                <span>{q.label}</span>
                <span className="text-muted-foreground">{q.used} / {q.total}</span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-md bg-muted">
                <div className="h-full rounded-md bg-primary" style={{ width: `${pct}%` }} />
              </div>
            </div>
          )
        })}
      </section>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">{t('settings.usage.details')}</div>
        <dl className="grid grid-cols-2 gap-y-2 text-sm">
          {detail.map((d) => (
            <div key={d.label} className="contents">
              <dt className="text-muted-foreground">{d.label}</dt>
              <dd className="text-foreground">{d.value}</dd>
            </div>
          ))}
        </dl>
      </section>
    </>
  )
}
