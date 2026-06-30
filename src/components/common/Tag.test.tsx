import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { Paperclip } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'

import { Tag } from './Tag'

describe('Tag', () => {
  it('renders a compact default tag', () => {
    render(<Tag>Ready</Tag>)

    const tag = screen.getByText('Ready')
    expect(tag).toHaveClass('inline-flex', 'h-5', 'rounded')
    expect(tag).toHaveClass('bg-muted', 'text-muted-foreground')
    expect(tag).toHaveClass('px-1.5', 'text-xs')
  })

  it('supports size and semantic color variants', () => {
    render(
      <>
        <Tag size="xs" color="primary">平台技能</Tag>
        <Tag size="md" color="warning" variant="outlined">需授权</Tag>
        <Tag color="destructive" variant="solid">失败</Tag>
      </>,
    )

    expect(screen.getByText('平台技能')).toHaveClass('h-[18px]', 'text-[10px]', 'bg-[rgba(var(--primary-rgb),0.10)]', 'text-primary')
    expect(screen.getByText('需授权')).toHaveClass('h-7', 'border-[rgba(var(--color-semantic-orange-rgb),0.40)]', 'bg-transparent', 'text-warning')
    expect(screen.getByText('失败')).toHaveClass('bg-destructive', 'text-destructive-foreground')
  })

  it('renders icons with consistent sizing', () => {
    render(<Tag icon={<Paperclip data-testid="paperclip" />}>附件</Tag>)

    expect(screen.getByTestId('paperclip')).toHaveClass('h-3.5', 'w-3.5', 'shrink-0')
    expect(screen.getByText('附件')).toBeInTheDocument()
  })

  it('supports close actions without triggering parent click', () => {
    const onClick = vi.fn()
    const onClose = vi.fn()

    render(
      <Tag asButton onClick={onClick} onClose={onClose} closeLabel="移除">
        文件.pdf
      </Tag>,
    )

    fireEvent.click(screen.getByRole('button', { name: '移除' }))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(onClick).not.toHaveBeenCalled()
  })

  it('can render an anchor through asChild', () => {
    render(
      <Tag asChild>
        <a href="https://example.com">example.com</a>
      </Tag>,
    )

    const link = screen.getByRole('link', { name: 'example.com' })
    expect(link).toHaveAttribute('href', 'https://example.com')
    expect(link).toHaveClass('inline-flex', 'h-5')
  })
})
