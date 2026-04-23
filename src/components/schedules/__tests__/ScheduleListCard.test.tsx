import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { ScheduleListCard } from '../ScheduleListCard'

describe('ScheduleListCard', () => {
  it('renders all three slots', () => {
    render(
      <ScheduleListCard
        header={<div>head</div>}
        table={<div>table</div>}
        empty={<div>empty</div>}
      />,
    )
    expect(screen.getByText('head')).toBeInTheDocument()
    expect(screen.getByText('table')).toBeInTheDocument()
    expect(screen.getByText('empty')).toBeInTheDocument()
  })
})
