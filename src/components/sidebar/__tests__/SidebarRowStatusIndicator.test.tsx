import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SidebarRowStatusIndicator } from '../SidebarRowStatusIndicator'

describe('SidebarRowStatusIndicator', () => {
  it('renders permission review chip copy', () => {
    render(<SidebarRowStatusIndicator status="permission-review" />)

    expect(screen.getByText('审核')).toBeInTheDocument()
    expect(screen.queryByLabelText('对话运行中')).not.toBeInTheDocument()
  })

  it('renders waiting reply chip copy', () => {
    render(<SidebarRowStatusIndicator status="waiting-reply" />)

    expect(screen.getByText('等待回复')).toBeInTheDocument()
    expect(screen.queryByLabelText('对话运行中')).not.toBeInTheDocument()
  })

  it('renders loader for loading status', () => {
    const { container } = render(<SidebarRowStatusIndicator status="loading" />)

    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
    expect(screen.getByLabelText('对话运行中')).toBeInTheDocument()
  })

  it('renders nothing for null status', () => {
    const { container } = render(<SidebarRowStatusIndicator status={null} />)

    expect(container.firstChild).toBeNull()
  })
})
