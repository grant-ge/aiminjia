import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Wallet, RefreshCw } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useBillingStore } from '@/stores/billingStore'

function formatDate(iso: string): string {
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

export function AccountBillingPanel() {
  const { t } = useTranslation()
  const summary = useBillingStore((s) => s.summary)
  const records = useBillingStore((s) => s.records)
  const pagination = useBillingStore((s) => s.pagination)
  const loadingSummary = useBillingStore((s) => s.loadingSummary)
  const loadingRecords = useBillingStore((s) => s.loadingRecords)
  const error = useBillingStore((s) => s.error)
  const refresh = useBillingStore((s) => s.refresh)
  const fetchRecords = useBillingStore((s) => s.fetchRecords)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const totalPages = Math.max(1, Math.ceil(pagination.total / pagination.size))
  const balanceNum = summary ? parseFloat(summary.balance) : 0
  const bonusAmountNum = summary ? parseFloat(summary.signup_bonus.amount) : 0
  const showBonus =
    !!summary?.signup_bonus.granted && balanceNum >= bonusAmountNum

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between border-b border-border pb-3">
        <div className="text-base font-semibold text-foreground">
          {t('settings.billing.title')}
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void refresh()}
          disabled={loadingSummary || loadingRecords}
          aria-label="refresh"
        >
          <RefreshCw className="h-4 w-4" />
        </Button>
      </div>

      {error && (
        <div className="rounded-md border border-destructive bg-destructive/5 px-3 py-2 text-sm text-destructive flex items-center justify-between gap-3">
          <span>{error}</span>
          <Button variant="ghost" size="sm" onClick={() => void refresh()}>
            {t('settings.billing.retry')}
          </Button>
        </div>
      )}

      <section className="grid grid-cols-3 gap-3">
        <div className="rounded-lg border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.balance')}
          </div>
          <div
            className={`mt-1 text-2xl font-semibold ${
              balanceNum < 0 ? 'text-destructive' : 'text-foreground'
            }`}
          >
            ¥{summary?.balance ?? '0.00'}
          </div>
          {showBonus && (
            <div className="mt-1 text-xs text-muted-foreground">
              {t('settings.billing.bonusHint')}
            </div>
          )}
        </div>
        <div className="rounded-lg border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.monthCost')}
          </div>
          <div className="mt-1 text-2xl font-semibold text-foreground">
            ¥{summary?.this_month.cost ?? '0.00'}
          </div>
        </div>
        <div className="rounded-lg border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.monthRequests')}
          </div>
          <div className="mt-1 text-2xl font-semibold text-foreground">
            {summary?.this_month.request_count ?? 0}
          </div>
        </div>
      </section>

      <section className="flex flex-col gap-3">
        <div className="text-sm font-semibold text-foreground">
          {t('settings.billing.records')}
        </div>
        {records.length === 0 && !loadingRecords ? (
          <div className="flex flex-col items-center gap-2 py-10 text-muted-foreground">
            <Wallet className="h-8 w-8" />
            <div className="text-sm">{t('settings.billing.empty')}</div>
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th className="py-2 pr-3 font-normal">
                  {t('settings.billing.cols.time')}
                </th>
                <th className="py-2 pr-3 font-normal">
                  {t('settings.billing.cols.type')}
                </th>
                <th className="py-2 pr-3 font-normal">
                  {t('settings.billing.cols.inputTokens')}
                </th>
                <th className="py-2 pr-3 font-normal">
                  {t('settings.billing.cols.outputTokens')}
                </th>
                <th className="py-2 font-normal">
                  {t('settings.billing.cols.cost')}
                </th>
              </tr>
            </thead>
            <tbody>
              {records.map((r) => (
                <tr key={r.id} className="border-b border-border/50">
                  <td className="py-2 pr-3 text-foreground">
                    {formatDate(r.created_at)}
                  </td>
                  <td className="py-2 pr-3 text-muted-foreground">
                    {r.request_type}
                  </td>
                  <td className="py-2 pr-3 text-muted-foreground">
                    {r.input_tokens}
                  </td>
                  <td className="py-2 pr-3 text-muted-foreground">
                    {r.output_tokens}
                  </td>
                  <td className="py-2 text-foreground">¥{r.cost}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        {totalPages > 1 && (
          <div className="flex items-center justify-center gap-1 pt-2">
            {Array.from({ length: totalPages }, (_, i) => i + 1).map((p) => (
              <Button
                key={p}
                size="sm"
                variant={p === pagination.page ? 'default' : 'ghost'}
                onClick={() => void fetchRecords(p)}
                disabled={loadingRecords}
              >
                {p}
              </Button>
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
