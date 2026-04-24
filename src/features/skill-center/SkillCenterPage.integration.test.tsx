import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: vi.fn() }),
}))

const HR_SKILL = {
  id: 'hr-analysis',
  displayName: 'HR分析',
  description: '详细描述',
  source: 'builtin',
  hasWorkflow: true,
  icon: 'users',
  category: 'hr',
  triggerText: '',
  shortDescription: '短描述',
  displayNameEn: 'HR Analysis',
  shortDescriptionEn: 'short',
}

const REC1 = { id: 'rec1', displayName: '推荐1', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r1', shortDescriptionEn: 's' }
const REC2 = { id: 'rec2', displayName: '推荐2', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r2', shortDescriptionEn: 's' }
const REC3 = { id: 'rec3', displayName: '推荐3', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r3', shortDescriptionEn: 's' }
const REC4 = { id: 'rec4', displayName: '推荐4', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r4', shortDescriptionEn: 's' }

describe('SkillCenterPage', () => {
  beforeEach(() => {
    useSkillStore.setState({
      skills: [REC1, REC2, REC3, REC4, HR_SKILL],
      recommendedIds: ['rec1', 'rec2', 'rec3', 'rec4'],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('顶栏渲染标题、技能数量徽章和搜索框', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
    expect(screen.getByText(/5 个技能/)).toBeInTheDocument()
    expect(screen.getByPlaceholderText('搜索技能名称或场景')).toBeInTheDocument()
  })

  it('顶栏有上传技能资料和创建技能按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.getByRole('button', { name: '上传技能资料' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /创建技能/ })).toBeInTheDocument()
  })

  it('分类 bar 包含全部/HR/财务/法务/销售/运营/通用', () => {
    render(<SkillCenterPage />)
    for (const label of ['全部', 'HR', '财务', '法务', '销售', '运营', '通用']) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument()
    }
  })

  it('热门推荐始终渲染，切换分类后也可见', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('热门推荐')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'HR' }))
    expect(screen.getByText('热门推荐')).toBeInTheDocument()
  })

  it('热门推荐显示 4 个推荐技能', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('推荐1')).toBeInTheDocument()
    expect(screen.getByText('推荐2')).toBeInTheDocument()
    expect(screen.getByText('推荐3')).toBeInTheDocument()
    expect(screen.getByText('推荐4')).toBeInTheDocument()
  })

  it('切换到 HR 分类后卡片点击进入详情', async () => {
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: 'HR' }))
    const cards = screen.getAllByTestId('skill-card')
    const hrCard = cards.find((c) => c.textContent?.includes('HR分析'))
    expect(hrCard).toBeTruthy()
    fireEvent.click(hrCard!)
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'hr-analysis' })
    })
  })

  it('没有常驻的详情/使用按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.queryByRole('button', { name: /^详情$/ })).toBeNull()
    expect(screen.queryByRole('button', { name: /^使用$/ })).toBeNull()
  })
})
