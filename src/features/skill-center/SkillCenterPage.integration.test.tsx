import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import i18n from '@/i18n'
import { SkillAlreadyExistsError, SkillValidationError, useSkillStore } from '@/stores/skillStore'
import { useAuthStore } from '@/stores/authStore'
import { useUiStore } from '@/stores/uiStore'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())
const openDialogMock = vi.hoisted(() => vi.fn())
const askDialogMock = vi.hoisted(() => vi.fn())
const tauriMock = vi.hoisted(() => ({
  listMarketplaceSkills: vi.fn(),
  refreshSkillRegistry: vi.fn().mockResolvedValue(undefined),
  syncBuiltinSkills: vi.fn().mockResolvedValue({ installed: [], updated: [], skipped: [], changed: [] }),
  listSkills: vi.fn().mockResolvedValue([]),
  installCustomSkill: vi.fn().mockResolvedValue('installed'),
  installMarketplaceSkill: vi.fn().mockResolvedValue('installed'),
  setSkillEnabled: vi.fn().mockResolvedValue(undefined),
  uninstallCustomSkill: vi.fn().mockResolvedValue('uninstalled'),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openDialogMock,
  ask: askDialogMock,
}))

vi.mock('@/lib/tauri', () => tauriMock)

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
const BID_WRITING = { id: 'bid-writing', displayName: '标书撰写工作流', description: 'd', source: 'builtin', hasWorkflow: false, icon: '', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'Bid Writing', shortDescriptionEn: 's', updatedAt: null, enabled: true }

const MARKET_NEW = { id: 101, pluginId: 'deep-research', name: '深入研究', description: '通过来源验证生成研究报告', category: 'general', icon: 'search', version: '1.0', scope: 'public', status: 'published', downloads: 22000, featured: true, packageSize: 128, tenantName: '', createdAt: '2026-06-10T00:00:00Z' }
const MARKET_ADDED = { id: 102, pluginId: 'local-report', name: '本地日报', description: '已添加的市场技能', category: 'ops', icon: 'file-text', version: '1.0', scope: 'tenant', status: 'published', downloads: 100, featured: false, packageSize: 128, tenantName: 'ACME', createdAt: '2026-06-10T00:00:00Z' }

function seedStore(extra?: Partial<ReturnType<typeof useSkillStore.getState>>) {
  useSkillStore.setState({
    skills: [REC1, REC2, REC3, REC4, CORE_SKILL, TENANT_SKILL, USER_SKILL],
    recommendedIds: ['rec1', 'rec2', 'rec3', 'rec4'],
    isLoading: false,
    reload: vi.fn().mockResolvedValue(undefined),
    upload: vi.fn().mockResolvedValue(undefined),
    installMarketplace: vi.fn().mockResolvedValue(undefined),
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
    tauriMock.listMarketplaceSkills.mockReset()
    tauriMock.listMarketplaceSkills.mockResolvedValue({
      items: [MARKET_NEW, MARKET_ADDED],
      total: 2,
      page: 1,
      size: 100,
    })
    seedStore()
    useAuthStore.setState({ isLoggedIn: true })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null, pendingSkill: null })
  })

  it('顶栏渲染标题、技能数量徽章和搜索框', () => {
    const { container } = render(<SkillCenterPage />)
    const topBar = container.querySelector('header[data-tauri-drag-region]')
    expect(topBar).toHaveClass('h-12')
    expect(topBar).not.toHaveClass('h-14')
    expect(topBar).not.toHaveClass('h-[45px]')
    expect(screen.getByText('技能中心')).toBeInTheDocument()
    expect(screen.queryByText(/7 个技能/)).toBeNull()
    expect(screen.getByRole('button', { name: '同步技能' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '已安装' })).toHaveTextContent('2')
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

  it('切换到内置后卡片点击打开详情弹窗', async () => {
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))
    const cards = screen.getAllByTestId('skill-card')
    const hrCard = cards.find((c) => c.textContent?.includes('创建技能'))
    expect(hrCard).toBeTruthy()
    fireEvent.click(hrCard!)
    expect(await screen.findByTestId('skill-detail-dialog')).toBeInTheDocument()
    expect(screen.getByTestId('skill-detail-dialog-title')).toHaveTextContent('创建技能')
    expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
    expect(screen.queryByText('正在加载技能详情...')).toBeNull()
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

  it('按技能 id 命中本地图标资源', () => {
    seedStore({ skills: [BID_WRITING], recommendedIds: ['bid-writing'] })

    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))

    const card = screen.getByTestId('skill-card')
    const image = card.querySelector('img')
    expect(image).toHaveAttribute('src', '/skill-avatars/bid-writing.jpg')
  })

  it('未命中图标时使用主题强调色 fallback', () => {
    seedStore({ skills: [REC1], recommendedIds: ['rec1'] })

    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))

    expect(screen.getByTestId('skill-card-avatar')).toHaveClass('bg-[rgba(var(--primary-rgb),0.10)]', 'text-primary')
    expect(screen.getByTestId('skill-card-fallback-avatar')).toHaveTextContent('推')
  })

  it('英文环境下用技能英文名称和简介渲染卡片', async () => {
    await i18n.changeLanguage('en-US')
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: '内置' }))

    expect(screen.getByText('Create Skill')).toBeInTheDocument()
    expect(screen.getByText('create skill')).toBeInTheDocument()
    expect(screen.queryByText('创建技能')).toBeNull()
  })

  it('加载中显示状态文案', async () => {
    tauriMock.listMarketplaceSkills.mockReturnValueOnce(new Promise(() => {}))
    seedStore({ skills: [], isLoading: false })

    render(<SkillCenterPage />)

    expect(await screen.findByText('正在加载技能...')).toBeInTheDocument()
  })

  it('加载失败显示错误和重试按钮', async () => {
    tauriMock.listMarketplaceSkills.mockRejectedValueOnce(new Error('market down'))
    const reload = vi.fn().mockResolvedValue(undefined)
    seedStore({ skills: [], reload })

    render(<SkillCenterPage />)

    await waitFor(() => expect(screen.getByText('技能加载失败')).toBeInTheDocument())
    expect(screen.getByText('market down')).toBeInTheDocument()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '重试' }))
    })
    expect(tauriMock.listMarketplaceSkills).toHaveBeenCalledTimes(2)
  })

  it('market view reads marketplace API and shows add or added only', async () => {
    render(<SkillCenterPage />)

    expect(await screen.findByText('深入研究')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加 深入研究' })).toBeInTheDocument()
    expect(screen.getByText('本地日报')).toBeInTheDocument()
    expect(screen.getByText('已添加')).toBeInTheDocument()
    expect(screen.queryByRole('switch')).toBeNull()
    expect(screen.queryByText('已关闭')).toBeNull()
    expect(screen.queryByText('去对话')).toBeNull()
  })

  it('市场技能没有本地图标资源时显示技能名首字 fallback', async () => {
    tauriMock.listMarketplaceSkills.mockResolvedValueOnce({
      items: [
        {
          ...MARKET_NEW,
          id: 401,
          pluginId: 'people-analytics',
          name: 'HR数据分析',
          category: 'hr',
          icon: 'file-text',
          version: '1.2',
        },
      ],
      total: 1,
      page: 1,
      size: 100,
    })
    const { container } = render(<SkillCenterPage />)

    await screen.findByText('HR数据分析')
    const card = container.querySelector('[data-aijia-skill-id="people-analytics"]')
    const avatar = card?.querySelector('[data-testid="skill-card-avatar"]')

    expect(avatar).toHaveClass('bg-[rgba(var(--primary-rgb),0.10)]', 'text-primary')
    expect(avatar).not.toHaveClass('bg-blue-500')
    expect(avatar?.querySelector('svg')).toBeNull()
    expect(avatar?.querySelector('[data-testid="skill-card-fallback-avatar"]')).toHaveTextContent('H')
  })

  it('点击未安装市场技能卡片打开无 header 的详情弹窗', async () => {
    const { container } = render(<SkillCenterPage />)

    await screen.findByText('深入研究')
    fireEvent.click(container.querySelector('[data-aijia-skill-id="deep-research"]')!)

    expect(screen.getByTestId('skill-detail-dialog')).toBeInTheDocument()
    expect(screen.getByTestId('skill-detail-dialog-body-viewport')).toHaveClass(
      'min-h-0',
      'overflow-auto',
      'max-h-[min(82vh,760px)]',
    )
    expect(screen.getByTestId('skill-detail-dialog-body')).toHaveClass('flex', 'flex-col', 'gap-6', 'py-6')
    expect(screen.getByTestId('skill-detail-dialog-footer')).toHaveClass('border-t', 'border-border')
    expect(screen.queryByTestId('skill-detail-dialog-header')).toBeNull()
    expect(screen.getByRole('button', { name: '安装 深入研究' })).toBeInTheDocument()
    expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
  })

  it('已安装市场技能弹窗 footer 使用按钮会准备 skill chip', async () => {
    const { container } = render(<SkillCenterPage />)

    await screen.findByText('本地日报')
    fireEvent.click(container.querySelector('[data-aijia-skill-id="local-report"]')!)
    fireEvent.click(screen.getByRole('button', { name: '使用' }))

    expect(useUiStore.getState().pendingSkill).toEqual({
      id: 'local-report',
      label: '本地日报',
      trigger: '/local-report',
    })
    expect(useUiStore.getState().route).toEqual({ kind: 'home' })
  })

  it('dedupes market packages by plugin id and keeps the best package card', async () => {
    tauriMock.listMarketplaceSkills.mockResolvedValueOnce({
      items: [
        { ...MARKET_NEW, id: 201, pluginId: 'html-ppt', name: 'html-ppt', version: '0.5', scope: 'public' },
        { ...MARKET_NEW, id: 202, pluginId: 'html-ppt', name: 'html-ppt', version: '0.6', scope: 'public' },
        { ...MARKET_NEW, id: 203, pluginId: 'html-ppt', name: 'html-ppt', version: '0.4', scope: 'public' },
        { ...MARKET_NEW, id: 204, pluginId: 'tenant-priority', name: 'Tenant Priority', version: '1.0', scope: 'tenant' },
        { ...MARKET_NEW, id: 205, pluginId: 'tenant-priority', name: 'Tenant Priority', version: '2.0', scope: 'public' },
      ],
      total: 5,
      page: 1,
      size: 100,
    })
    const { container } = render(<SkillCenterPage />)

    await waitFor(() => {
      expect(container.querySelectorAll('[data-aijia-skill-id="html-ppt"]').length).toBe(1)
    })
    const htmlPptCard = container.querySelector('[data-aijia-skill-id="html-ppt"]')
    expect(htmlPptCard?.querySelector('[data-testid="skill-card-version"]')?.textContent).toBe('0.6')

    const tenantCard = container.querySelector('[data-aijia-skill-id="tenant-priority"]')
    expect(tenantCard).toHaveAttribute('data-aijia-skill-source', 'tenant')
    expect(tenantCard?.querySelector('[data-testid="skill-card-version"]')?.textContent).toBe('1.0')

    fireEvent.click(container.querySelector('[data-aijia-skill-tab="builtin"]')!)

    await waitFor(() => expect(container.querySelector('[data-aijia-skill-id="create-skill"]')).toBeInTheDocument())
    expect(container.querySelector('[data-aijia-skill-id="html-ppt"]')).toBeNull()
    expect(container.querySelector('[data-aijia-skill-market-action="add"]')).toBeNull()
  })

  it('uses the installed local version on an already-added market card', async () => {
    seedStore({
      skills: [
        REC1,
        REC2,
        REC3,
        REC4,
        CORE_SKILL,
        TENANT_SKILL,
        USER_SKILL,
        {
          ...USER_SKILL,
          id: 'html-ppt',
          displayName: 'html-ppt',
          description: 'installed html ppt',
          version: '0.5',
          enabled: true,
        },
      ],
    })
    tauriMock.listMarketplaceSkills.mockResolvedValueOnce({
      items: [
        { ...MARKET_NEW, id: 301, pluginId: 'html-ppt', name: 'html-ppt', version: '0.6', scope: 'public' },
        { ...MARKET_NEW, id: 302, pluginId: 'html-ppt', name: 'html-ppt', version: '0.5', scope: 'public' },
      ],
      total: 2,
      page: 1,
      size: 100,
    })
    const { container } = render(<SkillCenterPage />)

    await waitFor(() => {
      expect(container.querySelectorAll('[data-aijia-skill-id="html-ppt"]').length).toBe(1)
    })
    const card = container.querySelector('[data-aijia-skill-id="html-ppt"]')
    expect(card).toHaveAttribute('data-aijia-skill-installed', 'true')
    expect(card?.querySelector('[data-testid="skill-card-version"]')?.textContent).toBe('0.5')
    expect(card?.querySelector('[data-testid="skill-card-source"]')).toBeNull()
    expect(card?.querySelector('[data-aijia-skill-market-action="added"]')).toBeInTheDocument()
    expect(card?.querySelector('[data-aijia-skill-market-action="add"]')).toBeNull()
  })

  it('market add installs package and stays on skill center', async () => {
    const installMarketplace = vi.fn().mockResolvedValue(undefined)
    seedStore({ installMarketplace })
    render(<SkillCenterPage />)

    const addButton = await screen.findByRole('button', { name: '添加 深入研究' })
    fireEvent.click(addButton)

    await waitFor(() => expect(installMarketplace).toHaveBeenCalledWith(101, 'deep-research'))
    expect(useUiStore.getState().pendingSkill).toBeNull()
    expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
  })

  it('已安装视图展示关闭开关并调用 setSkillEnabled', async () => {
    const setSkillEnabled = vi.fn().mockResolvedValue(undefined)
    seedStore({ setSkillEnabled })
    render(<SkillCenterPage />)

    fireEvent.click(screen.getByRole('button', { name: '已安装' }))
    const toggle = screen.getByRole('radiogroup', { name: '本地日报 技能开关' })
    fireEvent.click(within(toggle).getByRole('radio', { name: '开' }))

    await waitFor(() => expect(setSkillEnabled).toHaveBeenCalledWith('local-report', true))
  })

  it('已安装技能只有 source=user 显示自建，非 user 技能按市场处理', async () => {
    const platformInstalledSkill = {
      ...USER_SKILL,
      id: 'payslip',
      displayName: '玩转智能工资条',
      description: 'payslip CLI 操作助手',
      source: 'custom',
      version: '0.10',
      enabled: true,
    }
    seedStore({
      skills: [USER_SKILL, platformInstalledSkill],
      recommendedIds: [],
    })
    const { container } = render(<SkillCenterPage />)

    fireEvent.click(screen.getByRole('button', { name: '已安装' }))

    const userCard = container.querySelector('[data-aijia-skill-id="local-report"]')
    const platformCard = container.querySelector('[data-aijia-skill-id="payslip"]')

    expect(userCard?.querySelector('[data-testid="skill-card-source"]')).toHaveTextContent('自建')
    expect(platformCard?.querySelector('[data-testid="skill-card-source"]')).toHaveTextContent('市场')
    expect(platformCard?.querySelector('[data-testid="skill-card-source"]')).not.toHaveTextContent('自建')
    expect(platformCard?.querySelector('img')).toHaveAttribute('src', '/skill-avatars/smart-payslip.jpg')
    expect(platformCard?.querySelector('[data-testid="skill-card-fallback-avatar"]')).toBeNull()

    fireEvent.pointerDown(screen.getByRole('button', { name: '玩转智能工资条 更多操作' }))

    expect(screen.getByRole('menuitem', { name: '卸载技能' })).toBeInTheDocument()
    expect(screen.queryByRole('menuitem', { name: /导出/ })).toBeNull()
    expect(screen.queryByRole('menuitem', { name: '删除技能' })).toBeNull()
  })

  it('已安装技能 source=user 但命中市场 pluginId 时按市场安装处理', async () => {
    const marketInstalledUserSkill = {
      ...USER_SKILL,
      id: 'payslip',
      displayName: '玩转智能工资条',
      description: 'payslip CLI 操作助手',
      source: 'user',
      version: '0.10',
      enabled: true,
    }
    tauriMock.listMarketplaceSkills.mockResolvedValueOnce({
      items: [
        {
          ...MARKET_NEW,
          id: 501,
          pluginId: 'payslip',
          name: '玩转智能工资条',
          category: 'hr',
          icon: 'coins',
          version: '0.10',
        },
      ],
      total: 1,
      page: 1,
      size: 100,
    })
    seedStore({
      skills: [marketInstalledUserSkill],
      recommendedIds: [],
    })
    const { container } = render(<SkillCenterPage />)

    await screen.findByText('玩转智能工资条')
    fireEvent.click(screen.getByRole('button', { name: '已安装' }))

    const card = container.querySelector('[data-aijia-skill-id="payslip"]')
    expect(card?.querySelector('[data-testid="skill-card-source"]')).toHaveTextContent('市场')
    expect(card?.querySelector('[data-testid="skill-card-source"]')).not.toHaveTextContent('自建')
    expect(card?.querySelector('img')).toHaveAttribute('src', '/skill-avatars/smart-payslip.jpg')
    expect(card?.querySelector('[data-testid="skill-card-fallback-avatar"]')).toBeNull()

    fireEvent.pointerDown(screen.getByRole('button', { name: '玩转智能工资条 更多操作' }))
    expect(screen.getByRole('menuitem', { name: '卸载技能' })).toBeInTheDocument()
    expect(screen.queryByRole('menuitem', { name: /导出/ })).toBeNull()
    expect(screen.queryByRole('menuitem', { name: '删除技能' })).toBeNull()
  })

  it('已安装市场技能没有本地图标资源时才显示技能名首字 fallback', async () => {
    const marketInstalledUserSkill = {
      ...USER_SKILL,
      id: 'market-no-avatar',
      displayName: '市场无图标技能',
      description: '市场安装但没有本地图标资源',
      source: 'user',
      version: '1.0',
      enabled: true,
    }
    tauriMock.listMarketplaceSkills.mockResolvedValueOnce({
      items: [
        {
          ...MARKET_NEW,
          id: 601,
          pluginId: 'market-no-avatar',
          name: '市场无图标技能',
          category: 'general',
          icon: 'file-text',
          version: '1.0',
        },
      ],
      total: 1,
      page: 1,
      size: 100,
    })
    seedStore({
      skills: [marketInstalledUserSkill],
      recommendedIds: [],
    })
    const { container } = render(<SkillCenterPage />)

    await screen.findByText('市场无图标技能')
    fireEvent.click(screen.getByRole('button', { name: '已安装' }))

    const card = container.querySelector('[data-aijia-skill-id="market-no-avatar"]')
    expect(card?.querySelector('[data-testid="skill-card-source"]')).toHaveTextContent('市场')
    expect(card?.querySelector('img')).toBeNull()
    expect(card?.querySelector('[data-testid="skill-card-fallback-avatar"]')).toHaveTextContent('市')
  })
})
