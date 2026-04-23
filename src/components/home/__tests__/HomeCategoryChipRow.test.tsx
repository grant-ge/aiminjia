import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { HomeCategoryChipRow } from '../HomeCategoryChipRow'

const ITEMS = [
  { key: 'recommend', label: '为你推荐' },
  { key: 'writing', label: '文案有意' },
  { key: 'industry', label: '行业研究' },
]

describe('HomeCategoryChipRow', () => {
  it('renders all items', () => {
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    expect(screen.getByRole('button', { name: /为你推荐/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /行业研究/ })).toBeInTheDocument()
  })

  it('marks the active chip with brand-primary-subtle background', () => {
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={() => {}} />,
    )
    const active = screen.getByRole('button', { name: /为你推荐/ })
    expect(active.className).toMatch(/bg-brand-primary-subtle/)
  })

  it('calls onSelect with key on click', () => {
    const onSelect = vi.fn()
    render(
      <HomeCategoryChipRow items={ITEMS} activeKey="recommend" onSelect={onSelect} />,
    )
    fireEvent.click(screen.getByRole('button', { name: /行业研究/ }))
    expect(onSelect).toHaveBeenCalledWith('industry')
  })
})
