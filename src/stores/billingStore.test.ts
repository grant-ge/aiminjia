import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('@/lib/tauri', () => ({
  billingSummary: vi.fn(),
  billingUsageRecords: vi.fn(),
}))

import { billingSummary, billingUsageRecords } from '@/lib/tauri'
import { useBillingStore } from './billingStore'

const defaultFilters = {
  preset: 'thisMonth' as const,
  startDate: '2026-05-01',
  endDate: '2026-05-31',
  requestType: null,
  modelName: null,
}

function localStartIso(year: number, month: number, day: number) {
  return new Date(year, month - 1, day, 0, 0, 0, 0).toISOString()
}

function localEndIso(year: number, month: number, day: number) {
  return new Date(year, month - 1, day, 23, 59, 59, 999).toISOString()
}

describe('useBillingStore', () => {
  beforeEach(() => {
    useBillingStore.setState({
      summary: null,
      records: [],
      rangeSummary: { request_count: 0, input_tokens: 0, output_tokens: 0, cached_tokens: 0, cost: '0.00' },
      rangeSummaryPartial: false,
      filters: defaultFilters,
      pagination: { page: 1, size: 20, total: 0 },
      loadingSummary: false,
      loadingRecords: false,
      summaryError: false,
      recordsError: false,
      error: null,
    })
    vi.clearAllMocks()
  })

  it('fetchSummary loads balance and bonus into state', async () => {
    ;(billingSummary as any).mockResolvedValue({
      balance: '9.85',
      currency: 'CNY',
      this_month: { year_month: '2026-05', request_count: 2, input_tokens: 100, output_tokens: 50, cached_tokens: 0, cost: '0.15' },
      signup_bonus: { granted: true, amount: '10.00', granted_at: '2026-05-19T10:00:00+08:00' },
    })
    await useBillingStore.getState().fetchSummary()
    const s = useBillingStore.getState()
    expect(s.summary?.balance).toBe('9.85')
    expect(s.summary?.signup_bonus.granted).toBe(true)
    expect(s.loadingSummary).toBe(false)
    expect(s.error).toBeNull()
  })

  it('fetchRecords stores page and updates pagination', async () => {
    ;(billingUsageRecords as any).mockResolvedValue({
      page: 2,
      size: 20,
      total: 35,
      summary: {
        request_count: 35,
        input_tokens: 350,
        output_tokens: 175,
        cached_tokens: 0,
        cost: '0.350',
      },
      records: [{
        id: 1,
        created_at: '2026-05-19T14:00:00+08:00',
        request_type: 'chat',
        model_name: 'm',
        input_tokens: 10,
        output_tokens: 5,
        cached_tokens: 0,
        cost: '0.001',
        key_type: 'session',
      }],
    })
    await useBillingStore.getState().fetchRecords(2)
    const s = useBillingStore.getState()
    expect(s.pagination.page).toBe(2)
    expect(s.pagination.total).toBe(35)
    expect(s.records.length).toBe(1)
    expect(s.rangeSummary.request_count).toBe(35)
    expect(s.rangeSummaryPartial).toBe(false)
    expect(s.loadingRecords).toBe(false)
    expect(billingUsageRecords).toHaveBeenCalledWith(2, 20, {
      startAt: localStartIso(2026, 5, 1),
      endAt: localEndIso(2026, 5, 31),
      requestType: null,
      modelName: null,
    })
  })

  it('records error message on fetchSummary failure', async () => {
    ;(billingSummary as any).mockRejectedValue(new Error('network down'))
    await useBillingStore.getState().fetchSummary()
    const s = useBillingStore.getState()
    expect(s.error).toContain('network down')
    expect(s.summaryError).toBe(true)
    expect(s.loadingSummary).toBe(false)
    expect(s.summary).toBeNull()
  })

  it('records error message on fetchRecords failure', async () => {
    ;(billingUsageRecords as any).mockRejectedValue(new Error('boom'))
    await useBillingStore.getState().fetchRecords(1)
    const s = useBillingStore.getState()
    expect(s.error).toContain('boom')
    expect(s.recordsError).toBe(true)
    expect(s.loadingRecords).toBe(false)
  })

  it('refresh calls both endpoints in parallel', async () => {
    ;(billingSummary as any).mockResolvedValue({
      balance: '5.00', currency: 'CNY',
      this_month: { year_month: '2026-05', request_count: 0, input_tokens: 0, output_tokens: 0, cached_tokens: 0, cost: '0' },
      signup_bonus: { granted: true, amount: '10.00' },
    })
    ;(billingUsageRecords as any).mockResolvedValue({ page: 1, size: 20, total: 0, records: [] })
    await useBillingStore.getState().refresh()
    expect(billingSummary).toHaveBeenCalledTimes(1)
    expect(billingUsageRecords).toHaveBeenCalledTimes(1)
    expect(billingUsageRecords).toHaveBeenCalledWith(1, 20, {
      startAt: localStartIso(2026, 5, 1),
      endAt: localEndIso(2026, 5, 31),
      requestType: null,
      modelName: null,
    })
  })

  it('stores custom date range and uses it on record fetch', async () => {
    ;(billingUsageRecords as any).mockResolvedValue({ page: 1, size: 20, total: 0, records: [] })

    useBillingStore.getState().setCustomRange('2026-05-10', '2026-05-12')
    await useBillingStore.getState().fetchRecords(1)

    expect(useBillingStore.getState().filters).toMatchObject({
      preset: 'custom',
      startDate: '2026-05-10',
      endDate: '2026-05-12',
    })
    expect(billingUsageRecords).toHaveBeenCalledWith(1, 20, {
      startAt: localStartIso(2026, 5, 10),
      endAt: localEndIso(2026, 5, 12),
      requestType: null,
      modelName: null,
    })
  })

  it('sends request type and model filters when fetching records', async () => {
    ;(billingUsageRecords as any).mockResolvedValue({ page: 1, size: 20, total: 0, records: [] })

    useBillingStore.getState().setRecordFilters({
      requestType: 'chat',
      modelName: 'deepseek-reasoner',
    })
    await useBillingStore.getState().fetchRecords(1)

    expect(billingUsageRecords).toHaveBeenCalledWith(1, 20, {
      startAt: localStartIso(2026, 5, 1),
      endAt: localEndIso(2026, 5, 31),
      requestType: 'chat',
      modelName: 'deepseek-reasoner',
    })
  })

  it('keeps active search filters when changing pages', async () => {
    ;(billingUsageRecords as any).mockResolvedValue({ page: 3, size: 20, total: 80, records: [] })

    useBillingStore.getState().setRecordFilters({
      requestType: 'agent',
      modelName: 'qwen',
    })
    await useBillingStore.getState().fetchRecords(3)

    expect(useBillingStore.getState().pagination.page).toBe(3)
    expect(billingUsageRecords).toHaveBeenCalledWith(3, 20, expect.objectContaining({
      requestType: 'agent',
      modelName: 'qwen',
    }))
  })
})
