import { render, screen } from '@testing-library/react'
import '@testing-library/jest-dom'
import { describe, expect, it } from 'vitest'

import { SidebarCollapseFrame } from './SidebarCollapseFrame'

describe('SidebarCollapseFrame', () => {
  it('keeps the sidebar content mounted while expanded with width transition classes', () => {
    const { container } = render(
      <SidebarCollapseFrame hidden={false}>
        <button type="button">新任务</button>
      </SidebarCollapseFrame>,
    )

    const frame = container.querySelector('[data-aijia-sidebar-collapse-frame]')
    expect(frame).toHaveAttribute('data-state', 'expanded')
    expect(frame).toHaveAttribute('aria-hidden', 'false')
    expect(frame).toHaveClass('w-64', 'overflow-hidden', 'transition-[width]', 'duration-200', 'ease-out')
    expect(frame).not.toHaveAttribute('inert')
    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
  })

  it('collapses to zero width while keeping the sidebar content mounted but inert', () => {
    const { container } = render(
      <SidebarCollapseFrame hidden>
        <button type="button">新任务</button>
      </SidebarCollapseFrame>,
    )

    const frame = container.querySelector('[data-aijia-sidebar-collapse-frame]')
    expect(frame).toHaveAttribute('data-state', 'collapsed')
    expect(frame).toHaveAttribute('aria-hidden', 'true')
    expect(frame).toHaveAttribute('inert')
    expect(frame).toHaveClass('w-0', 'overflow-hidden', 'transition-[width]', 'duration-200', 'ease-out')
    expect(screen.getByRole('button', { name: '新任务', hidden: true })).toBeInTheDocument()
  })
})
