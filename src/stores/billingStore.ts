/**
 * Billing store — personal-tenant balance + usage records.
 *
 * Backed by `/v1/billing/{summary,usage-records}` on the tenant gateway via Tauri.
 * Used only when `tenant.type === 'personal'` (panel & menu hidden otherwise).
 */
import { create } from 'zustand'

import {
  billingSummary,
  billingUsageRecords,
  type BillingSummary,
  type UsageRecord,
} from '@/lib/tauri'

interface Pagination {
  page: number
  size: number
  total: number
}

interface BillingState {
  summary: BillingSummary | null
  records: UsageRecord[]
  pagination: Pagination
  loadingSummary: boolean
  loadingRecords: boolean
  error: string | null

  fetchSummary: () => Promise<void>
  fetchRecords: (page: number) => Promise<void>
  refresh: () => Promise<void>
  reset: () => void
}

const DEFAULT_PAGE_SIZE = 20
const EMPTY_BILLING_STATE = {
  summary: null,
  records: [] as UsageRecord[],
  pagination: { page: 1, size: DEFAULT_PAGE_SIZE, total: 0 },
  loadingSummary: false,
  loadingRecords: false,
  error: null,
}

export const useBillingStore = create<BillingState>((set, get) => ({
  ...EMPTY_BILLING_STATE,

  reset: () => set({ ...EMPTY_BILLING_STATE }),

  fetchSummary: async () => {
    set({ loadingSummary: true, error: null })
    try {
      const s = await billingSummary()
      set({ summary: s, loadingSummary: false })
    } catch (e) {
      set({ loadingSummary: false, error: e instanceof Error ? e.message : String(e) })
    }
  },

  fetchRecords: async (page: number) => {
    const { pagination } = get()
    set({ loadingRecords: true, error: null })
    try {
      const p = await billingUsageRecords(page, pagination.size)
      set({
        records: p.records,
        pagination: { page: p.page, size: p.size, total: p.total },
        loadingRecords: false,
      })
    } catch (e) {
      set({ loadingRecords: false, error: e instanceof Error ? e.message : String(e) })
    }
  },

  refresh: async () => {
    await Promise.all([get().fetchSummary(), get().fetchRecords(1)])
  },
}))
