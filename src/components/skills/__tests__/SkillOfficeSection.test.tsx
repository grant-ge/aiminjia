import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillOfficeSection } from '../SkillOfficeSection'

describe('SkillOfficeSection', () => {
  it('renders title 办公效率 and forwards slots', () => {
    render(
      <SkillOfficeSection categoryBar={<div data-testid="bar">bar</div>}>
        <div>card1</div>
      </SkillOfficeSection>,
    )
    expect(screen.getByText('办公效率')).toBeInTheDocument()
    expect(screen.getByTestId('bar')).toBeInTheDocument()
    expect(screen.getByText('card1')).toBeInTheDocument()
  })
})
