import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { GeneratedFileCard } from '../GeneratedFileCard'

describe('GeneratedFileCard', () => {
  it('renders title/sub/appName and fires onOpen', () => {
    const onOpen = vi.fn()
    render(
      <GeneratedFileCard
        title="绩效分析总结 · Q2"
        sub="Report · XLSX"
        appName="Microsoft Excel"
        onOpen={onOpen}
      />,
    )
    expect(screen.getByText('绩效分析总结 · Q2')).toBeInTheDocument()
    expect(screen.getByText('Report · XLSX')).toBeInTheDocument()
    const icon = screen.getByLabelText('XLS file icon')
    expect(icon).toBeInTheDocument()
    expect(icon).toHaveClass('-left-[5px]')
    expect(icon.querySelector('path')).toBeInTheDocument()
    expect(icon.parentElement).not.toHaveClass('overflow-hidden')
    expect(icon.closest('[data-testid="generated-file-card"]')).toHaveClass('h-16')
    fireEvent.click(screen.getByRole('button', { name: /Microsoft Excel/ }))
    expect(onOpen).toHaveBeenCalled()
  })
})
