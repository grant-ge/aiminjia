import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  employeeTemplateCatalog: vi.fn(async () => []),
  employeeTemplateRefresh: vi.fn(async () => 0),
}))

vi.mock('@/lib/tauri', () => ({
  employeeCreate: vi.fn(),
  employeeIndexKnowledgeAsync: vi.fn(),
  employeeTemplateCatalog: mocks.employeeTemplateCatalog,
  employeeTemplateRefresh: mocks.employeeTemplateRefresh,
}))

import { HireWizard } from './HireWizard'

describe('HireWizard', () => {
  beforeEach(() => {
    mocks.employeeTemplateCatalog.mockClear()
    mocks.employeeTemplateRefresh.mockClear()
  })

  it('lets users manually sync templates from the employee market', async () => {
    mocks.employeeTemplateRefresh.mockResolvedValueOnce(0).mockResolvedValueOnce(2)
    render(<HireWizard open onClose={() => {}} onHired={async () => {}} />)

    await waitFor(() => {
      expect(mocks.employeeTemplateRefresh).toHaveBeenCalledTimes(1)
    })
    mocks.employeeTemplateRefresh.mockClear()
    mocks.employeeTemplateCatalog.mockClear()

    fireEvent.click(screen.getByRole('button', { name: '同步服务端' }))

    await waitFor(() => {
      expect(mocks.employeeTemplateRefresh).toHaveBeenCalledTimes(1)
    })
    expect(mocks.employeeTemplateCatalog).toHaveBeenCalled()
  })
})
