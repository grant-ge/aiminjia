/**
 * Billing store — account balance + current-user usage records.
 *
 * Backed by personal billing Tauri commands.
 */
import { create } from 'zustand'

import {
  billingSummary,
  billingUsageRecords,
  type BillingSummary,
  type UsageRecordSummary,
  type UsageRecord,
} from '@/lib/tauri'

export type BillingRangePreset = 'today' | 'last7Days' | 'last30Days' | 'thisMonth' | 'lastMonth' | 'custom'

export interface BillingUsageFilters {
  preset: BillingRangePreset
  startDate: string
  endDate: string
  requestType: string | null
  modelName: string | null
}

interface Pagination {
  page: number
  size: number
  total: number
}

interface BillingState {
  summary: BillingSummary | null
  records: UsageRecord[]
  rangeSummary: UsageRecordSummary
  rangeSummaryPartial: boolean
  filters: BillingUsageFilters
  pagination: Pagination
  loadingSummary: boolean
  loadingRecords: boolean
  summaryError: boolean
  recordsError: boolean
  error: string | null

  fetchSummary: () => Promise<void>
  fetchRecords: (page: number) => Promise<void>
  setRangePreset: (preset: BillingRangePreset) => void
  setCustomRange: (startDate: string, endDate: string) => void
  setRecordFilters: (filters: Partial<Pick<BillingUsageFilters, 'requestType' | 'modelName'>>) => void
  refresh: () => Promise<void>
  reset: () => void
}

const DEFAULT_PAGE_SIZE = 20
const EMPTY_RANGE_SUMMARY: UsageRecordSummary = {
  request_count: 0,
  input_tokens: 0,
  output_tokens: 0,
  cached_tokens: 0,
  cost: '0.00',
}

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

function toDateInputValue(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function dateFromInput(value: string, endOfDay = false): Date | null {
  const [year, month, day] = value.split('-').map((part) => Number(part))
  if (!year || !month || !day) return null
  return endOfDay
    ? new Date(year, month - 1, day, 23, 59, 59, 999)
    : new Date(year, month - 1, day, 0, 0, 0, 0)
}

function dateInputToIso(value: string, endOfDay = false): string | null {
  return dateFromInput(value, endOfDay)?.toISOString() ?? null
}

function rangeForPreset(preset: BillingRangePreset, now = new Date()): Pick<BillingUsageFilters, 'startDate' | 'endDate'> {
  const start = new Date(now)
  const end = new Date(now)
  switch (preset) {
    case 'today':
      break
    case 'last7Days':
      start.setDate(start.getDate() - 6)
      break
    case 'last30Days':
      start.setDate(start.getDate() - 29)
      break
    case 'lastMonth':
      start.setFullYear(now.getFullYear(), now.getMonth() - 1, 1)
      end.setFullYear(now.getFullYear(), now.getMonth(), 0)
      break
    case 'thisMonth':
    case 'custom':
    default:
      start.setDate(1)
      break
  }
  return {
    startDate: toDateInputValue(start),
    endDate: toDateInputValue(end),
  }
}

function defaultFilters(): BillingUsageFilters {
  return {
    preset: 'thisMonth',
    ...rangeForPreset('thisMonth'),
    requestType: null,
    modelName: null,
  }
}

function summarizeRecords(records: UsageRecord[]): UsageRecordSummary {
  const summary = records.reduce(
    (acc, record) => {
      acc.request_count += 1
      acc.input_tokens += record.input_tokens
      acc.output_tokens += record.output_tokens
      acc.cached_tokens += record.cached_tokens
      acc.cost += Number.parseFloat(record.cost || '0')
      return acc
    },
    { request_count: 0, input_tokens: 0, output_tokens: 0, cached_tokens: 0, cost: 0 },
  )
  return {
    request_count: summary.request_count,
    input_tokens: summary.input_tokens,
    output_tokens: summary.output_tokens,
    cached_tokens: summary.cached_tokens,
    cost: summary.cost.toFixed(2),
  }
}

function sanitizeRange(startDate: string, endDate: string): Pick<BillingUsageFilters, 'startDate' | 'endDate'> {
  if (!startDate || !endDate) return rangeForPreset('thisMonth')
  return startDate <= endDate
    ? { startDate, endDate }
    : { startDate: endDate, endDate: startDate }
}

function requestFilters(filters: BillingUsageFilters) {
  return {
    startAt: dateInputToIso(filters.startDate),
    endAt: dateInputToIso(filters.endDate, true),
    requestType: filters.requestType,
    modelName: filters.modelName,
  }
}

function emptyBillingState() {
  return {
    summary: null,
    records: [] as UsageRecord[],
    rangeSummary: EMPTY_RANGE_SUMMARY,
    rangeSummaryPartial: false,
    filters: defaultFilters(),
    pagination: { page: 1, size: DEFAULT_PAGE_SIZE, total: 0 },
    loadingSummary: false,
    loadingRecords: false,
    summaryError: false,
    recordsError: false,
    error: null,
  }
}

export const useBillingStore = create<BillingState>((set, get) => ({
  ...emptyBillingState(),

  reset: () => set({ ...emptyBillingState() }),

  fetchSummary: async () => {
    set({ loadingSummary: true, summaryError: false, error: null })
    try {
      const s = await billingSummary()
      set({ summary: s, loadingSummary: false, summaryError: false })
    } catch (e) {
      set({
        loadingSummary: false,
        summaryError: true,
        error: e instanceof Error ? e.message : String(e),
      })
    }
  },

  fetchRecords: async (page: number) => {
    const { pagination, filters } = get()
    set({ loadingRecords: true, recordsError: false, error: null })
    try {
      const p = await billingUsageRecords(page, pagination.size, requestFilters(filters))
      set({
        records: p.records,
        rangeSummary: p.summary ?? summarizeRecords(p.records),
        rangeSummaryPartial: !p.summary && p.total > p.records.length,
        pagination: { page: p.page, size: p.size, total: p.total },
        loadingRecords: false,
        recordsError: false,
      })
    } catch (e) {
      set({
        loadingRecords: false,
        recordsError: true,
        error: e instanceof Error ? e.message : String(e),
      })
    }
  },

  setRangePreset: (preset) => {
    set((state) => ({
      filters: {
        ...state.filters,
        preset,
        ...rangeForPreset(preset),
      },
      pagination: { ...state.pagination, page: 1 },
    }))
  },

  setCustomRange: (startDate, endDate) => {
    const range = sanitizeRange(startDate, endDate)
    set((state) => ({
      filters: {
        ...state.filters,
        preset: 'custom',
        ...range,
      },
      pagination: { ...state.pagination, page: 1 },
    }))
  },

  setRecordFilters: (filters) => {
    set((state) => ({
      filters: {
        ...state.filters,
        requestType: filters.requestType === undefined ? state.filters.requestType : filters.requestType,
        modelName: filters.modelName === undefined ? state.filters.modelName : filters.modelName,
      },
      pagination: { ...state.pagination, page: 1 },
    }))
  },

  refresh: async () => {
    await Promise.all([get().fetchSummary(), get().fetchRecords(1)])
  },
}))
