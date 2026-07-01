import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Wallet, RefreshCw, Search, X } from 'lucide-react'

import { useBillingStore } from '@/stores/billingStore'
import type { BillingRangePreset } from '@/stores/billingStore'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { formatTokenCount } from '@/lib/format'

function formatDate(iso: string): string {
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

function formatCurrency(value: string | number | null | undefined): string {
  const amount = typeof value === 'number' ? value : Number.parseFloat(value ?? '0')
  return `¥${Number.isFinite(amount) ? amount.toFixed(2) : '0.00'}`
}

function formatCount(value: number): string {
  return new Intl.NumberFormat().format(value)
}

function averageCost(cost: string, count: number): string {
  if (count <= 0) return formatCurrency(0)
  return formatCurrency(Number.parseFloat(cost || '0') / count)
}

const RANGE_PRESETS: BillingRangePreset[] = [
  'today',
  'last7Days',
  'last30Days',
  'thisMonth',
  'lastMonth',
]

export function AccountBillingPanel() {
  const { t } = useTranslation()
  const summary = useBillingStore((s) => s.summary)
  const records = useBillingStore((s) => s.records)
  const rangeSummary = useBillingStore((s) => s.rangeSummary)
  const rangeSummaryPartial = useBillingStore((s) => s.rangeSummaryPartial)
  const filters = useBillingStore((s) => s.filters)
  const pagination = useBillingStore((s) => s.pagination)
  const loadingSummary = useBillingStore((s) => s.loadingSummary)
  const loadingRecords = useBillingStore((s) => s.loadingRecords)
  const summaryError = useBillingStore((s) => s.summaryError)
  const recordsError = useBillingStore((s) => s.recordsError)
  const refresh = useBillingStore((s) => s.refresh)
  const fetchRecords = useBillingStore((s) => s.fetchRecords)
  const setRangePreset = useBillingStore((s) => s.setRangePreset)
  const setCustomRange = useBillingStore((s) => s.setCustomRange)
  const setRecordFilters = useBillingStore((s) => s.setRecordFilters)
  const [modelNameDraft, setModelNameDraft] = useState(filters.modelName ?? '')

  useEffect(() => {
    if (filters.requestType) {
      setRecordFilters({ requestType: null })
    }
  }, [filters.requestType, setRecordFilters])

  useEffect(() => {
    void refresh()
  }, [refresh])

  useEffect(() => {
    setModelNameDraft(filters.modelName ?? '')
  }, [filters.modelName])

  const totalPages = Math.max(1, Math.ceil(pagination.total / pagination.size))
  const balanceNum = summary ? parseFloat(summary.balance) : 0
  const bonusAmountNum = summary ? parseFloat(summary.signup_bonus.amount) : 0
  const showBonus =
    !!summary?.signup_bonus.granted && balanceNum >= bonusAmountNum
  const totalTokens =
    rangeSummary.input_tokens + rangeSummary.output_tokens + rangeSummary.cached_tokens

  const handleRangePreset = (preset: BillingRangePreset) => {
    setRangePreset(preset)
    void fetchRecords(1)
  }

  const handleStartDateChange = (startDate: string) => {
    setCustomRange(startDate, filters.endDate)
    void fetchRecords(1)
  }

  const handleEndDateChange = (endDate: string) => {
    setCustomRange(filters.startDate, endDate)
    void fetchRecords(1)
  }

  const handleSearch = () => {
    setRecordFilters({
      requestType: null,
      modelName: modelNameDraft.trim() || null,
    })
    void fetchRecords(1)
  }

  const handleClearSearch = () => {
    setModelNameDraft('')
    setRecordFilters({ requestType: null, modelName: null })
    void fetchRecords(1)
  }

  const handleRefresh = () => {
    void refresh()
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between border-b border-border pb-3">
        <div className="text-base font-semibold text-foreground">
          {t('settings.billing.title')}
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={handleRefresh}
          disabled={loadingSummary || loadingRecords}
          aria-label="refresh"
          icon={<RefreshCw className="h-4 w-4" />}
        />
      </div>

      <section className="grid grid-cols-1 gap-3 sm:grid-cols-3 xl:grid-cols-4">
        <div className="rounded-md border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.balance')}
          </div>
          <div
            className={`mt-1 text-2xl font-semibold ${
              balanceNum < 0 ? 'text-destructive' : 'text-foreground'
            }`}
          >
            {summary ? formatCurrency(summary.balance) : t('settings.billing.unavailableValue')}
          </div>
          {showBonus && (
            <div className="mt-1 text-xs text-muted-foreground">
              {t('settings.billing.bonusHint')}
            </div>
          )}
          {summaryError && (
            <div className="mt-1 text-xs text-muted-foreground">
              {t('settings.billing.summaryUnavailable')}
            </div>
          )}
        </div>
        <div className="rounded-md border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.rangeCost')}
          </div>
          <div className="mt-1 text-2xl font-semibold text-foreground">
            {formatCurrency(rangeSummary.cost)}
          </div>
        </div>
        <div className="rounded-md border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.rangeTokens')}
          </div>
          <div className="mt-1 text-2xl font-semibold text-foreground">
            <span title={formatCount(totalTokens)}>{formatTokenCount(totalTokens)}</span>
          </div>
        </div>
        <div className="rounded-md border border-border bg-card p-4">
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.rangeRequests')}
          </div>
          <div className="mt-1 text-2xl font-semibold text-foreground">
            {formatCount(rangeSummary.request_count)}
          </div>
          <div className="mt-1 text-xs text-muted-foreground">
            {t('settings.billing.averageCost', {
              cost: averageCost(rangeSummary.cost, rangeSummary.request_count),
            })}
          </div>
        </div>
      </section>

      <section className="flex flex-col gap-3 border-b border-border pb-4">
        <div className="flex flex-wrap items-center gap-2">
          {RANGE_PRESETS.map((preset) => (
            <Button
              key={preset}
              type="button"
              size="sm"
              variant={filters.preset === preset ? 'secondary' : 'ghost'}
              onClick={() => handleRangePreset(preset)}
              disabled={loadingRecords}
            >
              {t(`settings.billing.range.${preset}`)}
            </Button>
          ))}
          <div className="ml-auto flex items-center gap-2">
            <Input
              type="date"
              aria-label={t('settings.billing.range.startDate')}
              value={filters.startDate}
              onChange={(event) => handleStartDateChange(event.currentTarget.value)}
              className="h-8 w-[136px] text-xs"
            />
            <span className="text-xs text-muted-foreground">
              {t('settings.billing.range.to')}
            </span>
            <Input
              type="date"
              aria-label={t('settings.billing.range.endDate')}
              value={filters.endDate}
              onChange={(event) => handleEndDateChange(event.currentTarget.value)}
              className="h-8 w-[136px] text-xs"
            />
          </div>
        </div>
        {rangeSummaryPartial ? (
          <div className="text-xs text-muted-foreground">
            {t('settings.billing.partialSummaryHint')}
          </div>
        ) : null}
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-3">
            <div className="text-sm font-semibold text-foreground">
              {t('settings.billing.records')}
            </div>
            <div className="text-xs text-muted-foreground">
              {t('settings.billing.totalRecords', { count: pagination.total })}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Input
              value={modelNameDraft}
              onChange={(event) => setModelNameDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') handleSearch()
              }}
              aria-label={t('settings.billing.search.model')}
              placeholder={t('settings.billing.search.modelPlaceholder')}
              className="h-8 w-[180px] text-xs"
            />
            <Button
              type="button"
              size="sm"
              variant="secondary"
              icon={<Search />}
              onClick={handleSearch}
              disabled={loadingRecords}
            >
              {t('settings.billing.search.apply')}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              icon={<X />}
              onClick={handleClearSearch}
              disabled={loadingRecords || !modelNameDraft}
            >
              {t('settings.billing.search.clear')}
            </Button>
          </div>
        </div>
        {recordsError && !loadingRecords ? (
          <div className="rounded-md border border-border bg-[rgba(var(--muted-rgb),0.30)] px-3 py-2 text-sm text-muted-foreground flex items-center justify-between gap-3">
            <span>{t('settings.billing.recordsUnavailable')}</span>
            <Button variant="ghost" size="sm" onClick={() => void fetchRecords(pagination.page)}>
              {t('settings.billing.retry')}
            </Button>
          </div>
        ) : records.length === 0 && !loadingRecords ? (
          <div className="flex flex-col items-center gap-2 py-10 text-muted-foreground">
            <Wallet className="h-8 w-8" />
            <div className="text-sm">{t('settings.billing.empty')}</div>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[640px] text-sm">
              <thead>
                <tr className="border-b border-border text-left text-xs text-muted-foreground">
                  <th className="py-2 pr-3 font-normal">
                    {t('settings.billing.cols.time')}
                  </th>
                  <th className="py-2 pr-3 text-right font-normal">
                    {t('settings.billing.cols.inputTokens')}
                  </th>
                  <th className="py-2 pr-3 text-right font-normal">
                    {t('settings.billing.cols.outputTokens')}
                  </th>
                  <th className="py-2 pr-3 text-right font-normal">
                    {t('settings.billing.cols.cachedTokens')}
                  </th>
                  <th className="py-2 pr-3 text-right font-normal">
                    {t('settings.billing.cols.totalTokens')}
                  </th>
                  <th className="py-2 text-right font-normal">
                    {t('settings.billing.cols.cost')}
                  </th>
                </tr>
              </thead>
              <tbody>
                {records.map((r) => {
                  const recordTokens = r.input_tokens + r.output_tokens + r.cached_tokens
                  return (
                    <tr key={r.id} className="border-b border-[rgba(var(--border-rgb),0.50)]">
                      <td className="py-2 pr-3 text-foreground">
                        {formatDate(r.created_at)}
                      </td>
                      <td className="py-2 pr-3 text-right text-muted-foreground" title={formatCount(r.input_tokens)}>
                        {formatTokenCount(r.input_tokens)}
                      </td>
                      <td className="py-2 pr-3 text-right text-muted-foreground" title={formatCount(r.output_tokens)}>
                        {formatTokenCount(r.output_tokens)}
                      </td>
                      <td className="py-2 pr-3 text-right text-muted-foreground" title={formatCount(r.cached_tokens)}>
                        {formatTokenCount(r.cached_tokens)}
                      </td>
                      <td className="py-2 pr-3 text-right text-muted-foreground" title={formatCount(recordTokens)}>
                        {formatTokenCount(recordTokens)}
                      </td>
                      <td className="py-2 text-right text-foreground">
                        {formatCurrency(r.cost)}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}

        {totalPages > 1 && (
          <div className="flex items-center justify-center gap-2 pt-2">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void fetchRecords(Math.max(1, pagination.page - 1))}
              disabled={loadingRecords || pagination.page <= 1}
            >
              {t('settings.billing.prevPage')}
            </Button>
            <span className="text-xs text-muted-foreground">
              {t('settings.billing.pageIndicator', {
                page: pagination.page,
                totalPages,
              })}
            </span>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void fetchRecords(Math.min(totalPages, pagination.page + 1))}
              disabled={loadingRecords || pagination.page >= totalPages}
            >
              {t('settings.billing.nextPage')}
            </Button>
          </div>
        )}
      </section>
    </div>
  )
}
