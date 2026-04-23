import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { Inbox, Loader2, CheckCircle2 } from 'lucide-react'
import { describe, expect, it } from 'vitest'

import { HomeStatusList } from '../HomeStatusList'

describe('HomeStatusList', () => {
  it('renders 3 rows with title and desc', () => {
    render(
      <HomeStatusList
        items={[
          { key: 'a', variant: 'empty', icon: <Inbox />, title: '空状态占位', desc: '...' },
          { key: 'b', variant: 'loading', icon: <Loader2 />, title: '加载状态占位', desc: '...' },
          { key: 'c', variant: 'success', icon: <CheckCircle2 />, title: '成功状态占位', desc: '...' },
        ]}
      />,
    )
    expect(screen.getByText('空状态占位')).toBeInTheDocument()
    expect(screen.getByText('加载状态占位')).toBeInTheDocument()
    expect(screen.getByText('成功状态占位')).toBeInTheDocument()
  })

  it('iconBox for empty uses brand-primary-subtle bg', () => {
    const { container } = render(
      <HomeStatusList
        items={[{ key: 'a', variant: 'empty', icon: <Inbox />, title: 't', desc: 'd' }]}
      />,
    )
    expect(
      container.querySelector('[data-testid="status-iconbox-a"]')?.className,
    ).toMatch(/bg-brand-primary-subtle/)
  })

  it('iconBox for success uses #DCFCE7 inline style', () => {
    const { container } = render(
      <HomeStatusList
        items={[{ key: 's', variant: 'success', icon: <CheckCircle2 />, title: 't', desc: 'd' }]}
      />,
    )
    const box = container.querySelector(
      '[data-testid="status-iconbox-s"]',
    ) as HTMLElement
    expect(box?.style.backgroundColor.toLowerCase()).toBe('rgb(220, 252, 231)')
  })
})
