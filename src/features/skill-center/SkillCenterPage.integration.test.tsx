import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
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
    createConversationFromSkillMock.mockClear()
    useSkillStore.setState({
      skills: [REC1, REC2, REC3, REC4, HR_SKILL],
      recommendedIds: ['rec1', 'rec2', 'rec3', 'rec4'],
      isLoading: false,
      reload: vi.fn().mockResolvedValue(undefined),
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('顶栏渲染标题、技能数量徽章和搜索框', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
    expect(screen.getByText(/5 个技能/)).toBeInTheDocument()
    expect(screen.getByPlaceholderText('搜索技能名称或场景')).toBeInTheDocument()
  })

  it('顶栏只保留导入技能按钮，不再有重复的上传技能资料按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.queryByRole('button', { name: '上传技能资料' })).toBeNull()
    expect(screen.getByRole('button', { name: /导入技能/ })).toBeInTheDocument()
  })

  it('点击导入技能会打开上传弹层，不再进入旧创建流程', () => {
    render(<SkillCenterPage />)

    fireEvent.click(screen.getByRole('button', { name: /导入技能/ }))

    expect(createConversationFromSkillMock).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog')).toHaveTextContent('上传技能')
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

  it('挂载后从后端刷新技能列表', async () => {
    useSkillStore.setState({ skills: [], recommendedIds: ['rec1'], isLoading: false })
    const reload = vi.fn().mockResolvedValue(undefined)
    useSkillStore.setState({ reload })

    render(<SkillCenterPage />)

    await waitFor(() => expect(reload).toHaveBeenCalled())
  })

  it('搜索框按名称或描述过滤技能', () => {
    render(<SkillCenterPage />)
    fireEvent.change(screen.getByPlaceholderText('搜索技能名称或场景'), { target: { value: 'HR' } })

    expect(screen.getByText('HR分析')).toBeInTheDocument()
    expect(screen.queryByText('推荐1')).toBeNull()
  })

  it('加载中显示状态文案', () => {
    useSkillStore.setState({ skills: [], isLoading: true, reload: vi.fn().mockResolvedValue(undefined) })

    render(<SkillCenterPage />)

    expect(screen.getByText('正在加载技能...')).toBeInTheDocument()
  })

  it('空列表显示空状态并支持重试', async () => {
    const reload = vi.fn().mockResolvedValue(undefined)
    useSkillStore.setState({ skills: [], isLoading: false, reload })

    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '重新加载' }))

    expect(screen.getByText('还没有可用技能')).toBeInTheDocument()
    await waitFor(() => expect(reload).toHaveBeenCalled())
  })

  it('加载失败显示错误和重试按钮', async () => {
    const reload = vi.fn().mockRejectedValue(new Error('backend down'))
    useSkillStore.setState({ skills: [], isLoading: false, reload })

    render(<SkillCenterPage />)

    await waitFor(() => expect(screen.getByText('技能加载失败')).toBeInTheDocument())
    expect(screen.getByText('backend down')).toBeInTheDocument()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '重试' }))
    })
    expect(reload).toHaveBeenCalledTimes(2)
  })
})
