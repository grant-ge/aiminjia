import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
}))

import { SkillDetailPage } from './SkillDetailPage'

describe('SkillDetailPage', () => {
  beforeEach(() => {
    void i18n.changeLanguage('zh-CN')
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

  it('renders the English skill name and description when language is English', async () => {
    await i18n.changeLanguage('en-US')

    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getAllByText('Business Proposal').length).toBeGreaterThan(0)
    expect(screen.getByText('Business proposal writing')).toBeInTheDocument()
    expect(screen.queryByText('商业方案撰写')).toBeNull()
  })
})
