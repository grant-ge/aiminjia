import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { SkillCategoryBar } from '../SkillCategoryBar'

describe('SkillCategoryBar', () => {
  it('marks active and fires onSelect', () => {
    const onSelect = vi.fn()
    render(
      <SkillCategoryBar
        items={[
          { key: 'a', label: 'A' },
          { key: 'b', label: 'B' },
        ]}
        activeKey="a"
        onSelect={onSelect}
      />,
    )
    const a = screen.getByRole('button', { name: 'A' })
    expect(a.className).toMatch(/bg-secondary|bg-card|bg-muted/)
    fireEvent.click(screen.getByRole('button', { name: 'B' }))
    expect(onSelect).toHaveBeenCalledWith('b')
  })
})
