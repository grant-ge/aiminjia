import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { EmployeeTemplateSnapshot, WorkplaceDirectoryResponse } from '@/lib/tauri'

const mocks = vi.hoisted(() => ({
  employeeTemplateCatalog: vi.fn<() => Promise<EmployeeTemplateSnapshot[]>>(async () => []),
  employeeTemplateRefresh: vi.fn<() => Promise<number>>(async () => 0),
  workplaceDirectoryCatalog: vi.fn<(
    lang?: string,
    options?: { forceRefresh?: boolean },
  ) => Promise<WorkplaceDirectoryResponse>>(
    async () => ({ schemaVersion: 1, categories: [], items: [] }),
  ),
}))

vi.mock('@/lib/tauri', () => ({
  employeeCreate: vi.fn(),
  employeeIndexKnowledgeAsync: vi.fn(),
  employeeTemplateCatalog: mocks.employeeTemplateCatalog,
  employeeTemplateRefresh: mocks.employeeTemplateRefresh,
  workplaceDirectoryCatalog: mocks.workplaceDirectoryCatalog,
}))

import { HireWizard } from './HireWizard'

function makeSnapshot(overrides: Partial<EmployeeTemplateSnapshot> = {}): EmployeeTemplateSnapshot {
  return {
    templateId: 'org:salary-expert',
    version: '1.0.0',
    name: '薪酬专家',
    avatar: '薪',
    role: '薪酬专家',
    description: '自动分析薪酬数据。',
    badge: '平台技能',
    systemPromptExtra: '',
    toolWhitelist: ['Skill'],
    cron: '',
    defaultSkillId: '',
    requiresDingtalk: false,
    requiresAttachment: null,
    resourceConfigSchema: null,
    resourceConfigUI: null,
    ...overrides,
  }
}

describe('HireWizard', () => {
  beforeEach(() => {
    mocks.employeeTemplateCatalog.mockReset()
    mocks.employeeTemplateCatalog.mockResolvedValue([])
    mocks.employeeTemplateRefresh.mockReset()
    mocks.employeeTemplateRefresh.mockResolvedValue(0)
    mocks.workplaceDirectoryCatalog.mockReset()
    mocks.workplaceDirectoryCatalog.mockResolvedValue({ schemaVersion: 1, categories: [], items: [] })
  })

  it('lets users manually sync templates from the employee market', async () => {
    mocks.employeeTemplateRefresh.mockResolvedValueOnce(0).mockResolvedValueOnce(2)
    render(<HireWizard open onClose={() => {}} onHired={async () => {}} />)

    await waitFor(() => {
      expect(mocks.employeeTemplateRefresh).toHaveBeenCalledTimes(1)
    })
    expect(mocks.workplaceDirectoryCatalog).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ forceRefresh: true }),
    )
    mocks.employeeTemplateRefresh.mockClear()
    mocks.employeeTemplateCatalog.mockClear()
    mocks.workplaceDirectoryCatalog.mockClear()

    fireEvent.click(screen.getByRole('button', { name: '更新内容' }))

    await waitFor(() => {
      expect(mocks.employeeTemplateRefresh).toHaveBeenCalledTimes(1)
    })
    expect(mocks.workplaceDirectoryCatalog).toHaveBeenCalledWith(
      expect.any(String),
      { forceRefresh: true },
    )
    expect(mocks.employeeTemplateCatalog).toHaveBeenCalled()
  })

  it('uses localized required skill names from the workplace directory', async () => {
    mocks.workplaceDirectoryCatalog.mockResolvedValueOnce({
      schemaVersion: 1,
      categories: [
        {
          categoryId: 'hr-admin',
          display: { name: '人事行政' },
          icon: 'users',
          color: '#6c5a9c',
          sortOrder: 10,
          resourceCount: 1,
        },
      ],
      items: [
        {
          resourceType: 'employee_template',
          resourceId: 'org:salary-expert',
          version: '1.0.0',
          workplaceCategoryId: 'hr-admin',
          display: { name: '薪酬专家', description: '自动分析薪酬数据。' },
          icon: '薪',
          requiredSkills: [
            {
              skillId: 'salary_analysis',
              source: 'platform',
              scope: 'public',
              display: { name: '薪酬分析' },
              versionRange: '',
            },
          ],
        },
      ],
    })
    mocks.employeeTemplateCatalog.mockResolvedValueOnce([makeSnapshot()])

    render(<HireWizard open onClose={() => {}} onHired={async () => {}} />)

    expect((await screen.findAllByText('薪酬专家')).length).toBeGreaterThan(0)
    expect(screen.getByText('薪酬分析')).toBeInTheDocument()
    expect(screen.getByText('人事行政')).toBeInTheDocument()
    expect(screen.queryByText('salary_analysis')).toBeNull()
    expect(mocks.employeeTemplateRefresh).not.toHaveBeenCalled()
  })
})
