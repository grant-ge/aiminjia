import '@testing-library/jest-dom'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'

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

describe('AccountBillingPanel', () => {
  beforeEach(() => {
    useBillingStore.setState({
      summary: null,
      records: [],
      pagination: { page: 1, size: 20, total: 0 },
      loadingSummary: false,
      loadingRecords: false,
      error: null,
    })
    vi.clearAllMocks()
  })

  it('renders balance and empty state after refresh', async () => {
    render(<AccountBillingPanel />)
    expect(await screen.findByText(/9\.85/)).toBeInTheDocument()
    expect(screen.getByText('settings.billing.empty')).toBeInTheDocument()
  })
})
