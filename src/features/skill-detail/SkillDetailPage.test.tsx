import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillDetailPage } from './SkillDetailPage'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
}))

const enabledSkill = {
  id: 'biz-proposal',
  displayName: '商业方案撰写',
  displayNameEn: 'Business Proposal',
  description: '依据业务数据生成商业方案。',
  source: 'user',
  hasWorkflow: true,
  icon: 'sparkles',
  shortDescription: '商业方案撰写',
  shortDescriptionEn: 'Business proposal writing',
  triggerText: '/biz-proposal',
  category: 'general',
  updatedAt: null,
  enabled: true,
}

describe('SkillDetailPage', () => {
  beforeEach(() => {
    void i18n.changeLanguage('zh-CN')
    createConversationFromSkillMock.mockClear()
    useSkillStore.setState({
      skills: [enabledSkill],
    })
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'biz-proposal' },
      settingsModal: null,
    })
  })

  it('uses the skill via the action bar without auto-running it', () => {
    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getAllByText('商业方案撰写').length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '使用' }))

    expect(createConversationFromSkillMock).not.toHaveBeenCalled()
    expect(useUiStore.getState().pendingSkill).toEqual({
      id: 'biz-proposal',
      label: '商业方案撰写',
      trigger: '/biz-proposal',
    })
    expect(useUiStore.getState().route).toEqual({ kind: 'home' })
  })

  it('renders the English skill name and description when language is English', async () => {
    await i18n.changeLanguage('en-US')

    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getAllByText('Business Proposal').length).toBeGreaterThan(0)
    expect(screen.getByText('Business proposal writing')).toBeInTheDocument()
    expect(screen.queryByText('商业方案撰写')).toBeNull()
  })

  it('disabled installed skill must be enabled before use', async () => {
    const setSkillEnabled = vi.fn().mockImplementation(async (skillId: string, enabled: boolean) => {
      useSkillStore.setState({
        skills: useSkillStore.getState().skills.map((skill) =>
          skill.id === skillId ? { ...skill, enabled } : skill,
        ),
      })
    })
    useSkillStore.setState({
      skills: [{ ...enabledSkill, enabled: false }],
      setSkillEnabled,
    })

    render(<SkillDetailPage skillId="biz-proposal" />)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '开启并使用' }))
    })

    expect(setSkillEnabled).toHaveBeenCalledWith('biz-proposal', true)
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'home' })
    })
  })
})
