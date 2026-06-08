import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCategoryBar } from '../SkillCategoryBar'

describe('SkillCategoryBar', () => {
  it('marks active chip with brand-primary-subtle class', () => {
    const onSelect = vi.fn()
    render(
      <SkillCategoryBar
        items={[
          { key: 'a', label: 'A' },
          { key: 'b', label: 'B' },
        ]}
        activeKey="a"
        onSelect={onSelect}
      />,
    )
    const a = screen.getByRole('button', { name: 'A' })
    expect(a.className).toMatch(/bg-brand-primary-subtle/)
    expect(a.className).toMatch(/text-primary/)
  })

  it('inactive chip has no bg-brand-primary-subtle', () => {
    render(
      <SkillCategoryBar
        items={[{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }]}
        activeKey="a"
        onSelect={() => {}}
      />,
    )
    const b = screen.getByRole('button', { name: 'B' })
    expect(b.className).not.toMatch(/bg-brand-primary-subtle/)
  })

  it('fires onSelect with correct key', () => {
    const onSelect = vi.fn()
    render(
      <SkillCategoryBar
        items={[{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }]}
        activeKey="a"
        onSelect={onSelect}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'B' }))
    expect(onSelect).toHaveBeenCalledWith('b')
  })

  it('keeps overflow discoverable when categories grow', () => {
    const { container } = render(
      <SkillCategoryBar
        items={[
          { key: 'a', label: 'A very long category name that should not stretch the page' },
          { key: 'b', label: 'B' },
        ]}
        activeKey="a"
        onSelect={() => {}}
      />,
    )
    const bar = container.firstElementChild
    expect(bar?.className).toMatch(/min-w-0/)
    expect(bar?.className).toMatch(/overflow-x-auto/)
    expect(bar?.className).not.toMatch(/scrollbar-width:none/)
    expect(bar?.className).not.toMatch(/webkit-scrollbar.*hidden/)
    expect(screen.getByRole('button', { name: /A very long category/ }).className).toMatch(/max-w-\[220px\]/)
  })
})
