import '@testing-library/jest-dom'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

const authMock = vi.hoisted(() => ({
  tenantType: 'personal',
}))

vi.mock('@/lib/tauri', () => ({
  billingSummary: vi.fn().mockResolvedValue({
    balance: '9.85',
    currency: 'CNY',
    this_month: { year_month: '2026-05', request_count: 2, input_tokens: 100, output_tokens: 50, cached_tokens: 0, cost: '0.15' },
    signup_bonus: { granted: true, amount: '10.00', granted_at: '2026-05-19T10:00:00+08:00' },
  }),
  billingUsageRecords: vi.fn().mockResolvedValue({ page: 1, size: 20, total: 0, records: [] }),
  enterpriseUsageRecords: vi.fn().mockResolvedValue({ page: 1, size: 20, total: 0, records: [] }),
}))

vi.mock('@/stores/authStore', () => ({
  useAuthStore: (sel: (s: unknown) => unknown) =>
    sel({
      tenant: { tenantType: authMock.tenantType },
    }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k }),
}))

import { AccountBillingPanel } from './AccountBillingPanel'
import { useBillingStore } from '@/stores/billingStore'
import { billingSummary, billingUsageRecords, enterpriseUsageRecords } from '@/lib/tauri'

describe('AccountBillingPanel', () => {
  beforeEach(() => {
    authMock.tenantType = 'personal'
    ;(billingSummary as any).mockResolvedValue({
      balance: '9.85',
      currency: 'CNY',
      this_month: { year_month: '2026-05', request_count: 2, input_tokens: 100, output_tokens: 50, cached_tokens: 0, cost: '0.15' },
      signup_bonus: { granted: true, amount: '10.00', granted_at: '2026-05-19T10:00:00+08:00' },
    })
    ;(billingUsageRecords as any).mockResolvedValue({ page: 1, size: 20, total: 0, records: [] })
    ;(enterpriseUsageRecords as any).mockResolvedValue({ page: 1, size: 20, total: 0, records: [] })
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

  it('applies request type and model search filters', async () => {
    const user = userEvent.setup()
    render(<AccountBillingPanel />)

    await screen.findByText(/9\.85/)
    vi.clearAllMocks()

    await user.type(screen.getByLabelText('settings.billing.search.type'), 'chat')
    await user.type(screen.getByLabelText('settings.billing.search.model'), 'reasoner')
    await user.click(screen.getByRole('button', { name: 'settings.billing.search.apply' }))

    expect(billingUsageRecords).toHaveBeenCalledTimes(1)
    expect(billingUsageRecords).toHaveBeenCalledWith(1, 20, expect.objectContaining({
      requestType: 'chat',
      modelName: 'reasoner',
    }))
  })

  it('keeps search filters when moving to the next page', async () => {
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
        requestType: 'chat',
        modelName: 'reasoner',
      },
    })

    render(<AccountBillingPanel />)

    await screen.findByText('deepseek-reasoner')
    vi.clearAllMocks()

    await user.click(screen.getByRole('button', { name: 'settings.billing.nextPage' }))

    expect(billingUsageRecords).toHaveBeenCalledWith(2, 20, expect.objectContaining({
      requestType: 'chat',
      modelName: 'reasoner',
    }))
  })

  it('uses neutral copy instead of raw disabled-account errors', async () => {
    ;(billingUsageRecords as any).mockRejectedValue(new Error('账户已被禁用'))

    render(<AccountBillingPanel />)

    expect(await screen.findByText('settings.billing.recordsUnavailable')).toBeInTheDocument()
    expect(screen.queryByText('账户已被禁用')).not.toBeInTheDocument()
  })

  it('does not call personal balance summary for enterprise tenants', async () => {
    authMock.tenantType = 'enterprise'

    render(<AccountBillingPanel />)

    await screen.findByText('settings.billing.empty')
    expect(enterpriseUsageRecords).toHaveBeenCalledTimes(1)
    expect(enterpriseUsageRecords).toHaveBeenCalledWith(1, 20, expect.objectContaining({
      requestType: null,
      modelName: null,
    }))
    expect(billingUsageRecords).not.toHaveBeenCalled()
    expect(billingSummary).not.toHaveBeenCalled()
    expect(screen.queryByText('settings.billing.balance')).not.toBeInTheDocument()
  })
})
