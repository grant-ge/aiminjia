import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillTokenView } from '../SkillTokenView'
import type { ComposerSkillToken } from '../types'

const skill: ComposerSkillToken = {
  id: 'html-ppt',
  label: 'html-ppt',
  command: '/html-ppt',
}

describe('SkillTokenView', () => {
  it('renders the composer skill token with shared tag sizing', () => {
    render(<SkillTokenView node={{ attrs: skill }} deleteNode={() => {}} />)

    const token = screen.getByText('html-ppt').closest('[data-skill-chip]')
    expect(token).toHaveClass('h-5', 'rounded', 'text-xs')
    expect(token).toHaveClass('skill-token-chip', 'text-primary')
    expect(screen.getByLabelText('skill')).toHaveClass('h-3.5', 'w-3.5')
  })

  it('removes the skill token from the composer', () => {
    const deleteNode = vi.fn()
    render(<SkillTokenView node={{ attrs: skill }} deleteNode={deleteNode} />)

    fireEvent.click(screen.getByRole('button', { name: 'remove skill html-ppt' }))

    expect(deleteNode).toHaveBeenCalledTimes(1)
  })
})
