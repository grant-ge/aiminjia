import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SchedulesPage } from './SchedulesPage'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

describe('SchedulesPage', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('loads scheduled jobs from backend and renders count', async () => {
    invokeMock.mockResolvedValueOnce([
      {
        id: 'sched-1',
        title: '日报汇总',
        prompt: '汇总昨日数据',
        cron: '0 9 * * *',
        humanSchedule: '每天 09:00',
        status: 'enabled',
        nextRunAt: '2026-04-26T01:00:00Z',
        timezone: 'Asia/Shanghai',
        createdAt: '2026-04-25T00:00:00Z',
        updatedAt: '2026-04-25T00:00:00Z',
      },
    ])

    render(<SchedulesPage />)

    expect(invokeMock).toHaveBeenCalledWith('list_schedules')
    expect(await screen.findByText('日报汇总')).toBeTruthy()
    expect(screen.getByText('每天 09:00')).toBeTruthy()
    expect(screen.getByText('已启用')).toBeTruthy()
    expect(screen.getByText('共 1 条')).toBeTruthy()
  })

  it('creates a schedule from template and refreshes list', async () => {
    invokeMock
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce({ id: 'sched-2' })
      .mockResolvedValueOnce([
        {
          id: 'sched-2',
          title: '门店巡检',
          prompt: '每周一汇总各门店巡检结果并生成报表。',
          cron: '0 9 * * 1',
          humanSchedule: '每周一 09:00',
          status: 'enabled',
          nextRunAt: null,
          timezone: 'Asia/Shanghai',
          createdAt: '2026-04-25T00:00:00Z',
          updatedAt: '2026-04-25T00:00:00Z',
        },
      ])

    render(<SchedulesPage />)
    await screen.findByText('还没有定时任务')

    fireEvent.click(screen.getAllByRole('button', { name: '使用模板' })[1])

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('create_schedule', {
        request: {
          title: '门店巡检',
          prompt: '每周一汇总各门店巡检结果并生成报表。',
          cron: '0 9 * * 1',
          timezone: 'Asia/Shanghai',
          enabled: true,
        },
      })
    })
    expect((await screen.findAllByText('门店巡检')).length).toBeGreaterThan(1)
  })
})
