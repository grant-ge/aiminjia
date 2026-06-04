import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { AssistantMarkdown } from '../AssistantMarkdown'

describe('AssistantMarkdown', () => {
  it('renders markdown text with the shared assistant typography', () => {
    const { container } = render(<AssistantMarkdown text="**重点**" />)

    expect(screen.getByText('重点')).toBeInTheDocument()
    expect(container.querySelector('strong')).toBeInTheDocument()
    expect(container.firstElementChild?.className).toContain('assistant-markdown')
    expect(container.firstElementChild?.className).toContain('text-[15px]')
    expect(container.firstElementChild?.className).toContain('leading-[1.65]')
  })

  it('renders nothing for blank text', () => {
    const { container } = render(<AssistantMarkdown text="   " />)

    expect(container.firstChild).toBeNull()
  })

  it('disableCodeHighlight=true → 不注入 hljs-* className', () => {
    const { container } = render(
      <AssistantMarkdown text={'```ts\nconst x = 1\n```'} disableCodeHighlight />,
    )
    const code = container.querySelector('pre code')
    expect(code).not.toBeNull()
    expect(code?.className ?? '').not.toMatch(/hljs/)
  })

  it('默认开启高亮（注入 hljs-* 或 language-* className）', () => {
    const { container } = render(
      <AssistantMarkdown text={'```ts\nconst x = 1\n```'} />,
    )
    const code = container.querySelector('pre code')
    expect(code).not.toBeNull()
    expect(code?.className ?? '').toMatch(/hljs|language-ts/)
  })
})
