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

  it('uses the skill via the action bar without auto-running it', () => {
    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getAllByText('商业方案撰写').length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '使用' }))

    // The redesigned "使用" button injects a pending skill chip and routes to
    // home; it must NOT auto-create/run a conversation.
    expect(createConversationFromSkillMock).not.toHaveBeenCalled()
    expect(useUiStore.getState().route).toEqual({ kind: 'home' })
  })
})
