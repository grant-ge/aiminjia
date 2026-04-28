import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { HomeCategoryChipRow } from '../HomeCategoryChipRow'

const ITEMS = [
  { key: 'recommend', label: '为你推荐', icon: 'sparkles' as const },
  { key: 'writing', label: '规划专家', icon: 'pencil' as const },
  { key: 'industry', label: '研究专家', icon: 'search' as const },
]

describe('HomeCategoryChipRow', () => {
  it('renders all items', () => {
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    expect(screen.getByRole('button', { name: /为你推荐/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /研究专家/ })).toBeInTheDocument()
  })

  it('marks the active chip with elevated text styling', () => {
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    const active = screen.getByRole('button', { name: /为你推荐/ })
    expect(active.className).toMatch(/font-semibold/)
    expect(active.className).toMatch(/text-foreground/)
  })

  it('calls onSelect with key on click', () => {
    const onSelect = vi.fn()
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={onSelect} />,
    )
    fireEvent.click(screen.getByRole('button', { name: /研究专家/ }))
    expect(onSelect).toHaveBeenCalledWith('industry')
  })

  it('uses space-between layout on the row container', () => {
    const { container } = render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    const row = container.firstElementChild
    expect(row?.className).toMatch(/justify-between/)
  })
})
