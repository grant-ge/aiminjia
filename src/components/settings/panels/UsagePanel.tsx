/**
 * @designSource design.pen#mbeKY planCard + BtLe0 quotaSec + SAOik detailSec
 */
interface QuotaItem { label: string; used: number; total: number }
interface DetailRow { label: string; value: string }

interface UsagePanelProps {
  planName: string
  planRenewLabel: string
  quota: QuotaItem[]
  detail: DetailRow[]
}

export function UsagePanel({ planName, planRenewLabel, quota, detail }: UsagePanelProps) {
  return (
    <>
      <div className="flex items-center justify-between border-b border-border py-4">
        <div className="flex flex-col gap-1">
          <div className="text-sm font-semibold text-foreground">{planName}</div>
          <div className="text-[0.8125rem] text-muted-foreground">{planRenewLabel}</div>
        </div>
      </div>
      <section className="flex flex-col gap-[18px]">
        {quota.map((q) => {
          const pct = Math.min(100, Math.round((q.used / Math.max(1, q.total)) * 100))
          return (
            <div key={q.label} className="flex flex-col gap-2">
              <div className="flex items-center justify-between text-[0.8125rem] text-foreground">
                <span>{q.label}</span>
                <span className="text-muted-foreground">{q.used} / {q.total}</span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
                <div className="h-full rounded-full bg-primary" style={{ width: `${pct}%` }} />
              </div>
            </div>
          )
        })}
      </section>
      <section className="flex flex-col gap-4">
        <div className="text-sm font-semibold text-foreground">用量明细</div>
        <dl className="grid grid-cols-2 gap-y-2 text-[0.8125rem]">
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
