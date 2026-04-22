import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/stores/uiStore', () => ({
  useUiStore: (selector: (state: { route: { kind: string }; settingsModal: null; setRoute: (route: unknown) => void; openSettings: () => void }) => unknown) =>
    selector({
      route: { kind: 'home' },
      settingsModal: null,
      setRoute: vi.fn(),
      openSettings: vi.fn(),
    }),
}))

import { Sidebar } from './Sidebar'

describe('Sidebar', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders current foundation sidebar skeleton', () => {
    render(<Sidebar onOpenSettings={vi.fn()} />)

    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument()
    expect(screen.getByPlaceholderText('搜索对话...')).toBeInTheDocument()
  })
})
