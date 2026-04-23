import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ProjectAccordion } from '../ProjectAccordion'

describe('ProjectAccordion', () => {
  it('shows children only when expanded', () => {
    const { rerender } = render(
      <ProjectAccordion name="默认项目" expanded={false} onToggle={() => {}}>
        <div>子项 A</div>
      </ProjectAccordion>,
    )
    expect(screen.queryByText('子项 A')).toBeNull()

    rerender(
      <ProjectAccordion name="默认项目" expanded onToggle={() => {}}>
        <div>子项 A</div>
      </ProjectAccordion>,
    )
    expect(screen.getByText('子项 A')).toBeInTheDocument()
  })

  it('invokes onToggle when header clicked', () => {
    const onToggle = vi.fn()
    render(
      <ProjectAccordion name="默认项目" expanded onToggle={onToggle}>
        <div>x</div>
      </ProjectAccordion>,
    )
    fireEvent.click(screen.getByRole('button', { name: /默认项目/ }))
    expect(onToggle).toHaveBeenCalled()
  })

  it('shows ChevronDown icon (rotates via expanded)', () => {
    const { container } = render(
      <ProjectAccordion name="X" expanded onToggle={() => {}}>
        <div />
      </ProjectAccordion>,
    )
    expect(container.querySelector('[data-icon="chevron-down"]')).toBeInTheDocument()
  })
})
