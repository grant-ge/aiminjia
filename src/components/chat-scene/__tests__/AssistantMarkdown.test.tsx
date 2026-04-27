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
    expect(container.firstElementChild?.className).toContain('leading-7')
  })

  it('renders nothing for blank text', () => {
    const { container } = render(<AssistantMarkdown text="   " />)

    expect(container.firstChild).toBeNull()
  })
})
