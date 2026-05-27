import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { EmployeeRecord } from '@/lib/tauri'
import { EmployeeCard } from './EmployeeCard'

function makeEmployee(overrides: Partial<EmployeeRecord> = {}): EmployeeRecord {
  return {
    id: 'emp-1',
    name: '小研',
    role: '行业调研员',
    description: '跟踪行业动态',
    avatar: '🔍',
    templateId: 'builtin:xiaoyuan',
    toolWhitelist: [],
    cron: null,
    timezone: 'Asia/Shanghai',
    lifecycle: 'active',
    cronEnabled: true,
    resourceConfig: {},
    systemPromptExtra: null,
    defaultSkillId: null,
    templateRef: {
      templateId: 'builtin:xiaoyuan',
      version: '1.2.0',
      sha256: 'abc',
      source: 'cache',
    },
    createdAt: '2026-05-27T00:00:00Z',
    updatedAt: '2026-05-27T00:00:00Z',
    lastRunAt: null,
    nextRunAt: null,
    ...overrides,
  }
}

describe('EmployeeCard', () => {
  it('shows the current template version when present', () => {
    render(
      <EmployeeCard
        employee={makeEmployee()}
        inboxEntries={[]}
        onClick={() => {}}
        onRefresh={vi.fn()}
      />,
    )

    expect(screen.getByText('v1.2.0')).toBeInTheDocument()
  })
})
