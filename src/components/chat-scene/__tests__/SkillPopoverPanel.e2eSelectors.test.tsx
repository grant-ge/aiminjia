import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillPopoverPanel } from '../SkillPopoverPanel'

const ITEMS = [
  { id: 'analysis', title: 'Analysis', subtitle: 'Analyze files', category: 'finance' },
  { id: 'proposal', title: 'Proposal', subtitle: 'Draft a proposal', category: 'general' },
]

describe('SkillPopoverPanel e2e selectors', () => {
  it('exposes stable selectors for skill picker items', () => {
    render(<SkillPopoverPanel items={ITEMS} onPick={() => {}} onClose={() => {}} />)

    const options = screen.getAllByRole('option')
    expect(screen.getByTestId('skill-popover-search')).toHaveAttribute('data-aijia-skill-picker-search')
    expect(options[0]).toHaveAttribute('data-aijia-skill-picker-item', 'true')
    expect(options[0]).toHaveAttribute('data-aijia-skill-id', 'analysis')
    expect(options[1]).toHaveAttribute('data-aijia-skill-picker-item', 'true')
    expect(options[1]).toHaveAttribute('data-aijia-skill-id', 'proposal')
  })
})
