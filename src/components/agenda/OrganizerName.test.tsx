import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { OrganizerName } from './OrganizerName'

const invokeMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

describe('OrganizerName', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    invokeMock.mockRejectedValue(new Error('missing employee'))
  })

  it('renders nothing for the default no-employee placeholder', () => {
    const { container } = render(<OrganizerName employeeId="default" />)

    expect(container).toBeEmptyDOMElement()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('renders the employee name when the organizer exists', async () => {
    invokeMock.mockResolvedValueOnce({
      id: 'emp-1',
      name: '小研',
    })

    render(<OrganizerName employeeId="emp-1" />)

    expect(await screen.findByText('@emp-1')).toBeInTheDocument()
    await waitFor(() => {
      expect(screen.getByText('@小研')).toBeInTheDocument()
    })
    expect(invokeMock).toHaveBeenCalledWith('employee_get', { id: 'emp-1' })
  })

  it('keeps a stale non-default organizer visible as unknown', async () => {
    invokeMock.mockRejectedValueOnce(new Error('missing employee'))

    render(<OrganizerName employeeId="emp-missing" />)

    await waitFor(() => {
      expect(screen.getByText('@未知员工')).toBeInTheDocument()
    })
  })
})
