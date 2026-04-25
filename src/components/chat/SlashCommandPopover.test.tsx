import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSkillStore } from '@/stores/skillStore'
import { SlashCommandPopover } from './SlashCommandPopover'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'zh-CN' },
    t: (_key: string, fallback?: string | { count?: number }) => {
      if (typeof fallback === 'string') return fallback
      if (_key === 'inputBar.slashFiltered') return `匹配 ${fallback?.count ?? 0} 个技能`
      if (_key === 'inputBar.slashEmpty') return '没有匹配的技能'
      return _key
    },
  }),
}))

describe('SlashCommandPopover', () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn()
    useSkillStore.setState({
      skills: [
        { id: 'salary-query', displayName: '薪酬查询', description: '', source: 'local', hasWorkflow: true, icon: '', category: 'general', triggerText: '/salary-query', shortDescription: '查询薪酬', displayNameEn: 'Salary Query', shortDescriptionEn: '' },
      ],
      recommendedIds: [],
      isLoading: false,
    })
  })

  it('shows local skills that match by id even without icon', () => {
    render(<SlashCommandPopover filterText="salary-query" onSelect={vi.fn()} onClose={vi.fn()} />)

    expect(screen.getByText('薪酬查询')).toBeInTheDocument()
    expect(screen.queryByText('没有匹配的技能')).not.toBeInTheDocument()
  })
})
