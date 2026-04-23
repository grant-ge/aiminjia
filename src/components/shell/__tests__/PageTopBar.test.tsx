import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { PageTopBar } from '../PageTopBar'

describe('PageTopBar', () => {
  it('default variant: empty bar with bottom border, h-14, px-6', () => {
    const { container } = render(<PageTopBar variant="default" />)
    const header = container.querySelector('header')
    expect(header?.className).toMatch(/h-14/)
    expect(header?.className).toMatch(/px-6/)
    expect(header?.className).toMatch(/border-b/)
  })

  it('title variant renders the title text', () => {
    render(<PageTopBar variant="title" title="技能中心" />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
  })

  it('breadcrumb variant renders provided crumbs', () => {
    render(
      <PageTopBar
        variant="breadcrumb"
        breadcrumbs={[{ label: 'A' }, { label: 'B' }]}
      />,
    )
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.getByText('B')).toBeInTheDocument()
  })

  it('compact variant uses smaller text class', () => {
    const { container } = render(<PageTopBar variant="compact" title="X" />)
    expect(container.querySelector('header')?.querySelector('div')?.className).toMatch(
      /text-sm/,
    )
  })

  it('renders trailing slot when provided', () => {
    render(
      <PageTopBar
        variant="title"
        title="X"
        trailing={<span>extra</span>}
      />,
    )
    expect(screen.getByText('extra')).toBeInTheDocument()
  })
})
