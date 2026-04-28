import '@testing-library/jest-dom'
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AssistantMarkdown } from '../../AssistantMarkdown'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (_key: string, fallback?: unknown) => {
      if (typeof fallback === 'string') return fallback
      if (fallback && typeof fallback === 'object' && 'defaultValue' in fallback) {
        return (fallback as { defaultValue: string }).defaultValue
      }
      return _key
    },
  }),
}))

vi.mock('@/lib/tauri', () => ({
  openFileByName: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: { getState: () => ({ push: vi.fn() }) },
}))

describe('AssistantMarkdown', () => {
  it('renders nothing for empty input', () => {
    const { container } = render(<AssistantMarkdown text="   " />)
    expect(container.firstChild).toBeNull()
  })

  it('renders a GFM table via TableView', () => {
    const md = `| Name | Qty |
|---|---|
| apple | 1 |
| banana | 2 |`
    render(<AssistantMarkdown text={md} />)
    expect(screen.getByTestId('table-view')).toBeInTheDocument()
    expect(screen.getByText('Name')).toBeInTheDocument()
    expect(screen.getByText('apple')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
  })

  it('strips raw HTML tags from input (skipHtml default)', () => {
    render(<AssistantMarkdown text={'plain <script>alert(1)</script> text'} />)
    expect(screen.queryByText(/script/)).not.toBeInTheDocument()
    expect(screen.getByText(/plain/)).toBeInTheDocument()
    expect(screen.getByText(/text/)).toBeInTheDocument()
  })

  it('renders fenced code blocks with a copy button', () => {
    render(<AssistantMarkdown text={'```js\nconst x = 1\n```'} />)
    expect(screen.getByText('js')).toBeInTheDocument()
    expect(screen.getByText('Copy')).toBeInTheDocument()
    expect(screen.getByText('const x = 1')).toBeInTheDocument()
  })

  it('renders inline code', () => {
    render(<AssistantMarkdown text={'use `npm install` to install'} />)
    expect(screen.getByText('npm install')).toBeInTheDocument()
  })

  it('renders bold and italic without escaping', () => {
    render(<AssistantMarkdown text={'**bold** and *italic*'} />)
    expect(screen.getByText('bold')).toBeInTheDocument()
    expect(screen.getByText('italic')).toBeInTheDocument()
  })

  it('renders standard markdown links', () => {
    render(<AssistantMarkdown text={'[Click](https://example.com)'} />)
    const link = screen.getByText('Click').closest('a') as HTMLAnchorElement
    expect(link).toBeTruthy()
    expect(link.href).toBe('https://example.com/')
  })

  it('renders empty GFM table (header only) as TableView with empty state', () => {
    const md = `| A | B |
|---|---|`
    render(<AssistantMarkdown text={md} />)
    expect(screen.getByTestId('table-view')).toBeInTheDocument()
    expect(screen.getByText('No data')).toBeInTheDocument()
  })
})
