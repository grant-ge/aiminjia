import '@testing-library/jest-dom'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { beforeEach, describe, it, expect, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
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
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    })
  })

  it('renders nothing for empty input', () => {
    const { container } = render(<AssistantMarkdown text="   " />)
    expect(container.firstChild).toBeNull()
  })

  it('renders a GFM table as native markdown table markup', () => {
    const md = `| Name | Qty |
|---|---|
| apple | 1 |
| banana | 2 |`
    const { container } = render(<AssistantMarkdown text={md} />)
    expect(screen.queryByTestId('table-view')).not.toBeInTheDocument()
    expect(container.querySelector('.markdown-table-wrap')).toBeInTheDocument()
    expect(container.querySelector('.markdown-table-scroll')).toBeInTheDocument()
    expect(container.querySelector('table')).toBeInTheDocument()
    expect(screen.getByTestId('markdown-table-copy-button')).toBeInTheDocument()
    expect(screen.getByText('Name')).toBeInTheDocument()
    expect(screen.getByText('apple')).toBeInTheDocument()
    expect(screen.getByText('2')).toBeInTheDocument()
  })


  it('copies markdown table content as CSV from the floating action', async () => {
    const md = `| Name | Qty |
|---|---|
| apple | 1 |`
    render(<AssistantMarkdown text={md} />)

    fireEvent.click(screen.getByTestId('markdown-table-copy-button'))

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('Name,Qty\r\napple,1\r\n')
    await waitFor(() => expect(screen.getByText('Copied')).toBeInTheDocument())
  })

  it('strips raw HTML tags from input (skipHtml default)', () => {
    render(<AssistantMarkdown text={'plain <script>alert(1)</script> text'} />)
    expect(screen.queryByText(/script/)).not.toBeInTheDocument()
    expect(screen.getByText(/plain/)).toBeInTheDocument()
    expect(screen.getByText(/text/)).toBeInTheDocument()
  })

  it('renders fenced code blocks with a copy button', () => {
    const { container } = render(<AssistantMarkdown text={'```js\nconst x = 1\n```'} />)
    expect(screen.getByText('js')).toBeInTheDocument()
    expect(screen.getByText('Copy')).toBeInTheDocument()
    expect(container.querySelector('code')?.textContent?.trim()).toBe('const x = 1')
    expect(container.querySelector('code.hljs.language-js')).toBeInTheDocument()
    expect(container.querySelector('.hljs-keyword')).toHaveTextContent('const')
    expect(container.querySelector('pre > div')).not.toBeInTheDocument()
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

  it('renders empty GFM table header without the structured TableView empty state', () => {
    const md = `| A | B |
|---|---|`
    const { container } = render(<AssistantMarkdown text={md} />)
    expect(screen.queryByTestId('table-view')).not.toBeInTheDocument()
    expect(container.querySelector('table')).toBeInTheDocument()
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.queryByText('No data')).not.toBeInTheDocument()
  })

  it('renders common markdown document structure tags for scoped typography', () => {
    const md = `# Main title

## Section title

### Subsection

Paragraph with **bold**, *italic*, ~~deleted~~, and \`inline code\`.

- First bullet
  - Nested bullet
- Second bullet

1. First step
2. Second step

> Quoted note

---`

    const { container } = render(<AssistantMarkdown text={md} />)

    expect(container.querySelector('.assistant-markdown')).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 1, name: 'Main title' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 2, name: 'Section title' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 3, name: 'Subsection' })).toBeInTheDocument()
    expect(container.querySelector('ul')).toBeInTheDocument()
    expect(container.querySelector('ol')).toBeInTheDocument()
    expect(container.querySelector('blockquote')).toHaveTextContent('Quoted note')
    expect(container.querySelector('hr')).toBeInTheDocument()
    expect(container.querySelector('del')).toHaveTextContent('deleted')
    expect(container.querySelector('code')).toHaveTextContent('inline code')
  })

  it('keeps normal markdown body text on foreground instead of primary accent', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf8')

    expect(css).toContain('.assistant-markdown {\n  color: var(--color-text-primary);')
  })

  it('prevents markdown tables from showing a vertical scrollbar beside the horizontal scrollbar', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf8')

    expect(css).toMatch(/\.assistant-markdown \.markdown-table-scroll \{[^}]*overflow-x: auto;[^}]*overflow-y: hidden;/s)
  })

  it('renders blockquotes as restrained quotes without card styling', () => {
    const css = readFileSync(resolve(process.cwd(), 'src/styles/globals.css'), 'utf8')

    expect(css).toMatch(/\.assistant-markdown blockquote \{[^}]*border-left: 3px solid/s)
    expect(css).toMatch(/\.assistant-markdown blockquote \{[^}]*border-radius: 0;/s)
    expect(css).toMatch(/\.assistant-markdown blockquote \{[^}]*background: transparent;/s)
  })

})
