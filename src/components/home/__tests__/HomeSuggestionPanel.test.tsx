import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { HomeSuggestionPanel } from '../HomeSuggestionPanel'

const ITEMS = [
  {
    key: 'a',
    title: '实施计划',
    desc: '帮我把这个项目目标拆成 4 个阶段，列出每阶段交付物、负责人和风险。',
    prompt: '...',
  },
  {
    key: 'b',
    title: '行业调研',
    desc: '围绕这个主题做一版调研摘要，包含趋势、竞品、机会点和建议动作。',
    prompt: '...',
  },
]

describe('HomeSuggestionPanel', () => {
  it('renders rows with title and desc', () => {
    render(<HomeSuggestionPanel items={ITEMS} onSelect={() => {}} />)

    expect(screen.getByRole('button', { name: /实施计划/ })).toBeInTheDocument()
    expect(screen.getByText(/围绕这个主题做一版调研摘要/)).toBeInTheDocument()
  })

  it('calls onSelect with the clicked suggestion item', () => {
    const onSelect = vi.fn()
    render(<HomeSuggestionPanel items={ITEMS} onSelect={onSelect} />)

    fireEvent.click(screen.getByRole('button', { name: /行业调研/ }))

    expect(onSelect).toHaveBeenCalledWith(ITEMS[1])
  })

  it('uses compact full-width spacing around the suggestion rows', () => {
    const { container } = render(<HomeSuggestionPanel items={ITEMS} onSelect={() => {}} />)
    const panel = container.firstElementChild

    expect(panel?.className).toMatch(/w-full/)
    expect(panel?.className).toMatch(/px-4/)
    expect(panel?.className).toMatch(/-mt-1\.5/)
  })
})
