import '@testing-library/jest-dom'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

vi.mock('@/lib/tauri', () => ({
  billingSummary: vi.fn().mockResolvedValue({
    balance: '9.85',
    currency: 'CNY',
    this_month: { year_month: '2026-05', request_count: 2, input_tokens: 100, output_tokens: 50, cached_tokens: 0, cost: '0.15' },
    signup_bonus: { granted: true, amount: '10.00', granted_at: '2026-05-19T10:00:00+08:00' },
  }),
  billingUsageRecords: vi.fn().mockResolvedValue({ page: 1, size: 20, total: 0, records: [] }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k }),
}))

import { AccountBillingPanel } from './AccountBillingPanel'
import { useBillingStore } from '@/stores/billingStore'
import { billingSummary, billingUsageRecords } from '@/lib/tauri'

describe('AccountBillingPanel', () => {
  beforeEach(() => {
    ;(billingSummary as any).mockResolvedValue({
      balance: '9.85',
      currency: 'CNY',
      this_month: { year_month: '2026-05', request_count: 2, input_tokens: 100, output_tokens: 50, cached_tokens: 0, cost: '0.15' },
      signup_bonus: { granted: true, amount: '10.00', granted_at: '2026-05-19T10:00:00+08:00' },
    })
    ;(billingUsageRecords as any).mockResolvedValue({ page: 1, size: 20, total: 0, records: [] })
    useBillingStore.setState({
      summary: null,
      records: [],
      rangeSummary: { request_count: 0, input_tokens: 0, output_tokens: 0, cached_tokens: 0, cost: '0.00' },
      rangeSummaryPartial: false,
      filters: {
        preset: 'thisMonth',
        startDate: '2026-05-01',
        endDate: '2026-05-31',
        requestType: null,
        modelName: null,
      },
      pagination: { page: 1, size: 20, total: 0 },
      loadingSummary: false,
      loadingRecords: false,
      summaryError: false,
      recordsError: false,
      error: null,
    })
    vi.clearAllMocks()
  })

  it('renders balance and empty state after refresh', async () => {
    render(<AccountBillingPanel />)
    expect(await screen.findByText(/9\.85/)).toBeInTheDocument()
    expect(screen.getByText('settings.billing.empty')).toBeInTheDocument()
  })

  it('refreshes records when a range preset is selected', async () => {
    const user = userEvent.setup()
    render(<AccountBillingPanel />)

    await screen.findByText(/9\.85/)
    vi.clearAllMocks()

    await user.click(screen.getByRole('button', { name: 'settings.billing.range.last7Days' }))

    expect(billingUsageRecords).toHaveBeenCalledTimes(1)
    expect(billingUsageRecords).toHaveBeenCalledWith(1, 20, expect.objectContaining({
      requestType: null,
      modelName: null,
    }))
  })

  it('applies model search filters without exposing request type search', async () => {
    const user = userEvent.setup()
    render(<AccountBillingPanel />)

    await screen.findByText(/9\.85/)
    vi.clearAllMocks()

    expect(screen.queryByLabelText('settings.billing.search.type')).not.toBeInTheDocument()
    await user.type(screen.getByLabelText('settings.billing.search.model'), 'reasoner')
    await user.click(screen.getByRole('button', { name: 'settings.billing.search.apply' }))

    expect(billingUsageRecords).toHaveBeenCalledTimes(1)
    expect(billingUsageRecords).toHaveBeenCalledWith(1, 20, expect.objectContaining({
      requestType: null,
      modelName: 'reasoner',
    }))
  })

  it('keeps model search filters when moving to the next page', async () => {
    const user = userEvent.setup()
    ;(billingUsageRecords as any).mockResolvedValue({
      page: 1,
      size: 20,
      total: 40,
      records: [{
        id: 1,
        created_at: '2026-05-19T14:00:00+08:00',
        request_type: 'chat',
        model_name: 'deepseek-reasoner',
        input_tokens: 10,
        output_tokens: 5,
        cached_tokens: 1,
        cost: '0.01',
        key_type: 'session',
      }],
    })
    useBillingStore.setState({
      filters: {
        preset: 'thisMonth',
        startDate: '2026-05-01',
        endDate: '2026-05-31',
        requestType: null,
        modelName: 'reasoner',
      },
    })

    render(<AccountBillingPanel />)

    await screen.findByRole('button', { name: 'settings.billing.nextPage' })
    vi.clearAllMocks()

    await user.click(screen.getByRole('button', { name: 'settings.billing.nextPage' }))

    expect(billingUsageRecords).toHaveBeenCalledWith(2, 20, expect.objectContaining({
      requestType: null,
      modelName: 'reasoner',
    }))
  })

  it('formats large token counts with readable units and hides type and model columns', async () => {
    ;(billingUsageRecords as any).mockResolvedValue({
      page: 1,
      size: 20,
      total: 1,
      summary: {
        request_count: 1,
        input_tokens: 129_000_000,
        output_tokens: 993_778,
        cached_tokens: 0,
        cost: '12.34',
      },
      records: [{
        id: 1,
        created_at: '2026-05-19T14:00:00+08:00',
        request_type: 'chat',
        model_name: 'deepseek-v4-flash',
        input_tokens: 129_000_000,
        output_tokens: 993_778,
        cached_tokens: 0,
        cost: '12.34',
        key_type: 'session',
      }],
    })

    render(<AccountBillingPanel />)

    expect(await screen.findAllByText('129.99 百万 Tokens')).not.toHaveLength(0)
    expect(screen.queryByText('settings.billing.cols.type')).not.toBeInTheDocument()
    expect(screen.queryByText('settings.billing.cols.model')).not.toBeInTheDocument()
    expect(screen.queryByText('chat')).not.toBeInTheDocument()
    expect(screen.queryByText('deepseek-v4-flash')).not.toBeInTheDocument()
  })

  it('uses neutral copy instead of raw disabled-account errors', async () => {
    ;(billingUsageRecords as any).mockRejectedValue(new Error('账户已被禁用'))

    render(<AccountBillingPanel />)

    expect(await screen.findByText('settings.billing.recordsUnavailable')).toBeInTheDocument()
    expect(screen.queryByText('账户已被禁用')).not.toBeInTheDocument()
  })
})
