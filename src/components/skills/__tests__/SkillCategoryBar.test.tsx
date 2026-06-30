import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCategoryBar } from '../SkillCategoryBar'

describe('SkillCategoryBar', () => {
  it('marks active chip with a quiet selected tab style', () => {
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
    expect(a).toHaveClass('rounded-md')
    expect(a).toHaveClass('bg-[rgba(var(--primary-rgb),0.10)]')
    expect(a).toHaveClass('text-primary')
    expect(a).toHaveClass('shadow-[inset_0_0_0_1px_rgba(var(--primary-rgb),0.12)]')
    expect(a).not.toHaveClass('bg-brand-primary-subtle')
    expect(a).not.toHaveClass('bg-[rgba(var(--muted-rgb),0.70)]')
    expect(a).not.toHaveClass('rounded-[10px]')
    expect(a).not.toHaveClass('rounded-lg')
  })

  it('inactive chip stays lightweight without selected background', () => {
    render(
      <SkillCategoryBar
        items={[{ key: 'a', label: 'A' }, { key: 'b', label: 'B' }]}
        activeKey="a"
        onSelect={() => {}}
      />,
    )
    const b = screen.getByRole('button', { name: 'B' })
    expect(b).toHaveClass('rounded-md')
    expect(b).toHaveClass('text-[rgba(var(--muted-foreground-rgb),0.80)]')
    expect(b).toHaveClass('hover:bg-[rgba(var(--muted-rgb),0.40)]')
    expect(b.className).not.toMatch(/bg-brand-primary-subtle/)
    expect(b).not.toHaveClass('bg-[rgba(var(--primary-rgb),0.10)]')
    expect(b).not.toHaveClass('rounded-[10px]')
    expect(b).not.toHaveClass('rounded-lg')
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
    expect(bar).toHaveClass('gap-2')
    expect(bar).toHaveClass('px-1')
    expect(bar).toHaveClass('pb-1')
    expect(bar).not.toHaveClass('p-1')
    expect(bar).not.toHaveClass('pb-2')
    expect(bar).not.toHaveClass('bg-card')
    expect(bar).not.toHaveClass('rounded-md')
    expect(bar).not.toHaveClass('border')
    expect(bar).not.toHaveClass('border-border')
    expect(bar?.className).not.toMatch(/scrollbar-width:none/)
    expect(bar?.className).not.toMatch(/webkit-scrollbar.*hidden/)
    expect(screen.getByRole('button', { name: /A very long category/ }).className).toMatch(/max-w-\[220px\]/)
  })
})
