import '@testing-library/jest-dom'
import { render, screen, fireEvent } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { HomeSkillCenterPill } from '../HomeSkillCenterPill'

describe('HomeSkillCenterPill', () => {
  it('renders label and fires onClick', () => {
    const onClick = vi.fn()
    render(<HomeSkillCenterPill onClick={onClick} />)
    fireEvent.click(screen.getByRole('button', { name: /前往技能中心/ }))
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})
