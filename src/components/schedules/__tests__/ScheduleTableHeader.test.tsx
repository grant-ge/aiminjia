import { describe, expect, it } from 'vitest'

import { SCHEDULE_TABLE_GRID_COLUMNS } from '../ScheduleTableHeader'

describe('ScheduleTableHeader', () => {
  it('keeps the trailing action column visible inside a narrow list pane', () => {
    expect(SCHEDULE_TABLE_GRID_COLUMNS).toContain('minmax(0,')
    expect(SCHEDULE_TABLE_GRID_COLUMNS).toContain('max-content')
    expect(SCHEDULE_TABLE_GRID_COLUMNS).not.toContain('20rem')
    expect(SCHEDULE_TABLE_GRID_COLUMNS).not.toContain('13rem')
  })
})
