import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillHotSection } from '../SkillHotSection'

describe('SkillHotSection', () => {
  it('renders title 热门推荐 and the children grid', () => {
    render(
      <SkillHotSection>
        <div>cardA</div>
      </SkillHotSection>,
    )
    expect(screen.getByText('热门推荐')).toBeInTheDocument()
    expect(screen.getByText('cardA')).toBeInTheDocument()
  })
})
