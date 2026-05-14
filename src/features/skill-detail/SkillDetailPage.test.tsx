import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
}))

import { SkillDetailPage } from './SkillDetailPage'

describe('SkillDetailPage', () => {
  beforeEach(() => {
    createConversationFromSkillMock.mockClear()
    useSkillStore.setState({
      skills: [
        {
          id: 'biz-proposal',
          displayName: '商业方案撰写',
          displayNameEn: 'Business Proposal',
          description: '依据业务数据生成商业方案。',
          source: 'custom',
          hasWorkflow: true,
          icon: 'sparkles',
          shortDescription: '商业方案撰写',
          shortDescriptionEn: 'Business proposal writing',
          triggerText: '/biz-proposal',
          category: 'general',
          updatedAt: null,
        },
      ],
    })
    useUiStore.setState({ route: { kind: 'skill-detail', skillId: 'biz-proposal' }, settingsModal: null })
  })

  it('renders try items without click-to-run behavior', () => {
    render(<SkillDetailPage skillId="biz-proposal" />)

    const cards = screen.getAllByTestId('skill-card')
    expect(cards).toHaveLength(3)
    fireEvent.click(cards[0])

    expect(createConversationFromSkillMock).not.toHaveBeenCalled()
    expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'biz-proposal' })
  })
})
