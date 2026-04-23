import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SkillMetaRow } from '../SkillMetaRow'

describe('SkillMetaRow', () => {
  it('renders all label/value pairs', () => {
    render(
      <SkillMetaRow
        items={[
          { label: '来源', value: 'AI 小家内置' },
          { label: '更新时间', value: '2026-04-20' },
        ]}
      />,
    )
    expect(screen.getByText('来源')).toBeInTheDocument()
    expect(screen.getByText('AI 小家内置')).toBeInTheDocument()
    expect(screen.getByText('更新时间')).toBeInTheDocument()
    expect(screen.getByText('2026-04-20')).toBeInTheDocument()
  })
})
