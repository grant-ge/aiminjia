import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import i18n from '@/i18n'
import { SkillAlreadyExistsError, SkillValidationError, useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())
const openDialogMock = vi.hoisted(() => vi.fn())
const askDialogMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openDialogMock,
  ask: askDialogMock,
}))

const CORE_SKILL = {
  id: 'create-skill',
  displayName: '创建技能',
  description: '创建 AI 小家技能',
  source: 'global',
  hasWorkflow: true,
  icon: 'tool',
  category: 'general',
  triggerText: '/create-skill',
  shortDescription: '创建技能',
  displayNameEn: 'Create Skill',
  shortDescriptionEn: 'create skill',
  updatedAt: null,
  enabled: true,
}

const REC1 = { id: 'rec1', displayName: '推荐1', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r1', shortDescriptionEn: 's', updatedAt: null, enabled: true }
const REC2 = { id: 'rec2', displayName: '推荐2', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r2', shortDescriptionEn: 's', updatedAt: null, enabled: true }
const REC3 = { id: 'rec3', displayName: '推荐3', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r3', shortDescriptionEn: 's', updatedAt: null, enabled: true }
const REC4 = { id: 'rec4', displayName: '推荐4', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r4', shortDescriptionEn: 's', updatedAt: null, enabled: true }
const TENANT_SKILL = { id: 'tenant-policy', displayName: '企业制度问答', description: '企业下发', source: 'tenant', hasWorkflow: false, icon: 'building', category: 'general', triggerText: '/tenant-policy', shortDescription: '制度问答', displayNameEn: 'Policy Q&A', shortDescriptionEn: 'policy', updatedAt: null, enabled: false }
const USER_SKILL = { id: 'local-report', displayName: '本地日报', description: '本地导入', source: 'user', hasWorkflow: false, icon: 'file-text', category: 'ops', triggerText: '/local-report', shortDescription: '本地日报', displayNameEn: 'Local Report', shortDescriptionEn: 'local report', updatedAt: null, enabled: false }

function seedStore(extra?: Partial<ReturnType<typeof useSkillStore.getState>>) {
  useSkillStore.setState({
    skills: [REC1, REC2, REC3, REC4, CORE_SKILL, TENANT_SKILL, USER_SKILL],
    recommendedIds: ['rec1', 'rec2', 'rec3', 'rec4'],
    isLoading: false,
    reload: vi.fn().mockResolvedValue(undefined),
    upload: vi.fn().mockResolvedValue(undefined),
    uninstall: vi.fn().mockResolvedValue(undefined),
    setSkillEnabled: vi.fn().mockResolvedValue(undefined),
    ...extra,
  })
}

describe('SkillCenterPage', () => {
  beforeEach(() => {
    void i18n.changeLanguage('zh-CN')
    createConversationFromSkillMock.mockClear()
    openDialogMock.mockReset()
    askDialogMock.mockReset()
    seedStore()
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('顶栏渲染标题、技能数量徽章和搜索框', () => {
    const { container } = render(<SkillCenterPage />)
    const topBar = container.querySelector('header[data-tauri-drag-region]')
    expect(topBar).toHaveClass('h-14')
    expect(topBar).not.toHaveClass('h-[45px]')
    expect(screen.getByText('技能中心')).toBeInTheDocument()
    expect(screen.getByText(/7 个技能/)).toBeInTheDocument()
    expect(screen.getByPlaceholderText('搜索技能名称或场景')).toBeInTheDocument()
  })

  it('顶栏只有一个导入技能按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.queryByRole('button', { name: '上传技能资料' })).toBeNull()
    expect(screen.getByRole('button', { name: /导入技能/ })).toBeInTheDocument()
  })

  it('点击「+ 导入技能」走 directory picker', async () => {
    openDialogMock.mockResolvedValueOnce(null)
    render(<SkillCenterPage />)

    fireEvent.pointerDown(screen.getByRole('button', { name: /导入技能/ }))
    fireEvent.click(screen.getByRole('menuitem', { name: '导入技能目录' }))

    await waitFor(() => expect(openDialogMock).toHaveBeenCalled())
    expect(openDialogMock).toHaveBeenCalledWith(
      expect.objectContaining({ directory: true, multiple: false }),
    )
  })

  it('upload 抛出 SkillValidationError 时弹校验结果对话框', async () => {
    openDialogMock.mockResolvedValueOnce('/tmp/bad-skill')
    const upload = vi.fn().mockRejectedValueOnce(
      new SkillValidationError({ kind: 'missingSkillMd' }),
    )
    seedStore({ upload })

    render(<SkillCenterPage />)
    await act(async () => {
      fireEvent.pointerDown(screen.getByRole('button', { name: /导入技能/ }))
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('menuitem', { name: '导入技能目录' }))
    })

    await waitFor(() => expect(screen.getByText('技能目录不符合规范')).toBeInTheDocument())
    expect(screen.getByText(/未找到 SKILL.md/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重新选择目录' })).toBeInTheDocument()
  })

  it('upload 抛出 parseFailed 时校验对话框透传 detail', async () => {
    openDialogMock.mockResolvedValueOnce('/tmp/bad-skill')
    const upload = vi.fn().mockRejectedValueOnce(
      new SkillValidationError({ kind: 'parseFailed', detail: 'yaml: invalid token' }),
    )
    seedStore({ upload })

    render(<SkillCenterPage />)
    await act(async () => {
      fireEvent.pointerDown(screen.getByRole('button', { name: /导入技能/ }))
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('menuitem', { name: '导入技能目录' }))
    })

    await waitFor(() => expect(screen.getByText('技能目录不符合规范')).toBeInTheDocument())
    expect(screen.getByText(/yaml: invalid token/)).toBeInTheDocument()
  })

  it('upload 抛出 alreadyExists 时走覆盖确认 (不弹校验结果对话框)', async () => {
    openDialogMock.mockResolvedValueOnce('/tmp/dup-skill')
    askDialogMock.mockResolvedValueOnce(false)
    const upload = vi.fn().mockRejectedValueOnce(new SkillAlreadyExistsError('dup-skill'))
    seedStore({ upload })

    render(<SkillCenterPage />)
    await act(async () => {
      fireEvent.pointerDown(screen.getByRole('button', { name: /导入技能/ }))
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('menuitem', { name: '导入技能目录' }))
    })

    await waitFor(() => expect(askDialogMock).toHaveBeenCalled())
    expect(screen.queryByText('技能目录不符合规范')).toBeNull()
  })

  it('分类 bar 包含市场/内置/已安装', () => {
    render(<SkillCenterPage />)
    for (const label of ['市场', '内置', '已安装']) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument()
    }
  })

  it('切换到内置后卡片点击进入详情', async () => {
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))
    const cards = screen.getAllByTestId('skill-card')
    const hrCard = cards.find((c) => c.textContent?.includes('创建技能'))
    expect(hrCard).toBeTruthy()
    fireEvent.click(hrCard!)
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'create-skill' })
    })
  })

  it('挂载后从后端刷新技能列表', async () => {
    const reload = vi.fn().mockResolvedValue(undefined)
    seedStore({ skills: [], recommendedIds: ['rec1'], reload })

    render(<SkillCenterPage />)

    await waitFor(() => expect(reload).toHaveBeenCalled())
  })

  it('搜索框按名称或描述过滤技能', () => {
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))
    fireEvent.change(screen.getByPlaceholderText('搜索技能名称或场景'), { target: { value: 'Create' } })

    expect(screen.getAllByText('创建技能').length).toBeGreaterThan(0)
    expect(screen.queryByText('推荐1')).toBeNull()
  })

  it('英文环境下用技能英文名称和简介渲染卡片', async () => {
    await i18n.changeLanguage('en-US')
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))

    expect(screen.getByText('Create Skill')).toBeInTheDocument()
    expect(screen.getByText('create skill')).toBeInTheDocument()
    expect(screen.queryByText('创建技能')).toBeNull()
  })

  it('加载中显示状态文案', () => {
    seedStore({ skills: [], isLoading: true })

    render(<SkillCenterPage />)

    expect(screen.getByText('正在加载技能...')).toBeInTheDocument()
  })

  it('加载失败显示错误和重试按钮', async () => {
    const reload = vi.fn().mockRejectedValue(new Error('backend down'))
    seedStore({ skills: [], reload })

    render(<SkillCenterPage />)

    await waitFor(() => expect(screen.getByText('技能加载失败')).toBeInTheDocument())
    expect(screen.getByText('backend down')).toBeInTheDocument()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '重试' }))
    })
    expect(reload).toHaveBeenCalledTimes(2)
  })

  it('市场视图只提供添加使用入口，不展示关闭开关', () => {
    render(<SkillCenterPage />)

    expect(screen.getByText('企业制度问答')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加并使用 企业制度问答' })).toBeInTheDocument()
    expect(screen.queryByRole('switch', { name: /企业制度问答/ })).toBeNull()
    expect(screen.queryByText('已关闭')).toBeNull()
  })

  it('已安装视图展示关闭开关并调用 setSkillEnabled', async () => {
    const setSkillEnabled = vi.fn().mockResolvedValue(undefined)
    seedStore({ setSkillEnabled })
    render(<SkillCenterPage />)

    fireEvent.click(screen.getByRole('button', { name: '已安装' }))
    fireEvent.click(screen.getByRole('switch', { name: '本地日报 技能开关' }))

    await waitFor(() => expect(setSkillEnabled).toHaveBeenCalledWith('local-report', true))
  })
})
