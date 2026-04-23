import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const createConversationFromSkill = vi.fn()

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    createConversationFromSkill,
  }),
}))

describe('SkillCenterPage', () => {
  beforeEach(() => {
    useSkillStore.setState({
      skills: [
        {
          id: 'writing-plans',
          displayName: '写计划',
          description: 'desc',
          source: 'builtin',
          hasWorkflow: true,
          icon: 'file-text',
          category: 'dev',
          triggerText: '',
          shortDescription: '短描述',
          displayNameEn: 'Plan',
          shortDescriptionEn: 'short',
        },
      ],
      recommendedIds: ['writing-plans'],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('切换分类并点击卡片进入详情', async () => {
    render(<SkillCenterPage />)

    fireEvent.click(screen.getByRole('button', { name: '开发' }))
    fireEvent.click(screen.getAllByRole('button', { name: '详情' })[0])

    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'writing-plans' })
    })
  })
})
