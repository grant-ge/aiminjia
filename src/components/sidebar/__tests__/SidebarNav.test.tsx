import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SidebarNav } from '../SidebarNav'

describe('SidebarNav', () => {
  it('renders nav items for 新任务, 数字员工, 专家团, 技能中心, 定时任务 and IM 频道', () => {
    render(<SidebarNav activeKey="home" onSelect={() => {}} />)
    expect(screen.getByRole('button', { name: '新任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '数字员工' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '专家团' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '技能中心' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '定时任务' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'IM 频道' })).toBeInTheDocument()
  })

  it('marks the active item with sidebar-accent background class', () => {
    render(<SidebarNav activeKey="skill-center" onSelect={() => {}} />)
    const active = screen.getByRole('button', { name: '技能中心' })
    expect(active.className).toMatch(/bg-sidebar-accent/)
  })

  it('renders each nav item at the compact 32px height', () => {
    render(<SidebarNav activeKey="home" onSelect={() => {}} />)
    for (const nav of screen.getAllByRole('button')) {
      expect(nav).toHaveClass('h-8')
      expect(nav).not.toHaveClass('h-9')
    }
  })

  it('calls onSelect with the kind on click', () => {
    const onSelect = vi.fn()
    render(<SidebarNav activeKey="home" onSelect={onSelect} />)
    screen.getByRole('button', { name: '数字员工' }).click()
    expect(onSelect).toHaveBeenCalledWith('employees')
  })
})
